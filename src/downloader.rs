use anyhow::Result;
use reqwest::{Client, ClientBuilder, StatusCode};
use std::collections::{HashSet, HashMap, BinaryHeap};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use std::path::{Path, PathBuf};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::cmp::Ordering;

use crate::file_manager::FileManager;
use crate::html_parser::{HtmlParser, ResourceType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadPriority {
    Critical = 0,    // CSS and JavaScript files
    High = 1,        // HTML pages
    Normal = 2,      // Images and other resources
}

impl PartialOrd for DownloadPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DownloadPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        // Lower numbers = higher priority
        match (self, other) {
            (DownloadPriority::Critical, DownloadPriority::Critical) => Ordering::Equal,
            (DownloadPriority::Critical, _) => Ordering::Less,
            (DownloadPriority::High, DownloadPriority::Critical) => Ordering::Greater,
            (DownloadPriority::High, DownloadPriority::High) => Ordering::Equal,
            (DownloadPriority::High, _) => Ordering::Less,
            (DownloadPriority::Normal, DownloadPriority::Normal) => Ordering::Equal,
            (DownloadPriority::Normal, _) => Ordering::Greater,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DownloadTask {
    pub url: String,
    pub depth: usize,
    pub priority: DownloadPriority,
    pub resource_type: Option<ResourceType>,
}

impl Ord for DownloadTask {
    fn cmp(&self, other: &Self) -> Ordering {
        // First compare by priority (lower number = higher priority)
        let priority_cmp = self.priority.cmp(&other.priority);
        if priority_cmp != Ordering::Equal {
            return priority_cmp;
        }
        
        // If priorities are equal, lower depth = higher priority
        other.depth.cmp(&self.depth)
    }
}

impl PartialOrd for DownloadTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct WebsiteMirror {
    base_url: String,
    output_dir: PathBuf,
    max_depth: usize,
    max_concurrent: usize,
    ignore_robots: bool,
    download_external: bool,
    only_resources: Option<Vec<String>>,
    client: Client,
    file_manager: FileManager,
    html_parser: HtmlParser,
    visited_urls: Arc<Mutex<HashSet<String>>>,
    download_queue: Arc<Mutex<BinaryHeap<DownloadTask>>>,
    semaphore: Arc<Semaphore>,
    download_cache: Arc<Mutex<HashMap<String, String>>>, // URL -> local path mapping
}

impl WebsiteMirror {
    /// Check if a resource type should be processed based on the only_resources filter
    fn should_process_resource_type(&self, resource_type: &ResourceType) -> bool {
        if let Some(ref only_resources) = self.only_resources {
            let type_str = match resource_type {
                ResourceType::Image => "images",
                ResourceType::CSS => "css",
                ResourceType::JavaScript => "js",
                ResourceType::Link => "html",
                ResourceType::Other => "other",
            };
            only_resources.iter().any(|r| r.to_lowercase() == type_str)
        } else {
            // If no filter specified, process all resource types
            true
        }
    }

    pub fn new(
        base_url: &str,
        output_dir: &Path,
        max_depth: usize,
        max_concurrent: usize,
        ignore_robots: bool,
        download_external: bool,
        only_resources: Option<Vec<String>>,
    ) -> Result<Self> {
        let client = Self::build_http_client()?;
        let file_manager = FileManager::new(output_dir)?;
        let html_parser = HtmlParser::new(base_url)?;
        
        Ok(Self {
            base_url: base_url.to_string(),
            output_dir: output_dir.to_path_buf(),
            max_depth,
            max_concurrent,
            ignore_robots,
            download_external,
            only_resources,
            client,
            file_manager,
            html_parser,
            visited_urls: Arc::new(Mutex::new(HashSet::new())),
            download_queue: Arc::new(Mutex::new(BinaryHeap::new())),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            download_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }
    
    fn build_http_client() -> Result<Client> {
        // Build a simple HTTP client with default SSL handling
        let client = ClientBuilder::new()
            .use_rustls_tls()
            .user_agent("WebsiteMirror/1.0")
            .timeout(std::time::Duration::from_secs(480))
            .build()?;
        
        Ok(client)
    }
    
    pub async fn mirror_website(&mut self) -> Result<()> {
        println!("🚀 Starting website mirroring for: {}", self.base_url.blue());
        println!("📁 Output directory: {:?}", self.output_dir);
        println!("🔗 Max depth: {}", self.max_depth);
        println!("⚡ Max concurrent downloads: {}", self.max_concurrent);
        
        // Add the base URL to the download queue with high priority (HTML page)
        // Only add HTML pages if we're not filtering to specific resource types
        if self.only_resources.is_none() || self.should_process_resource_type(&ResourceType::Link) {
            let mut queue = self.download_queue.lock().unwrap();
            queue.push(DownloadTask {
                url: self.base_url.clone(),
                depth: 0,
                priority: DownloadPriority::High,
                resource_type: None,
            });
        } else {
            println!("🔍 Resource filter active: skipping HTML page crawling");
        }
        
        let progress_bar = ProgressBar::new_spinner();
        progress_bar.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner} {msg}")
                .unwrap()
        );
        
        // Process the download queue
        loop {
            let download_task = {
                let mut queue = self.download_queue.lock().unwrap();
                queue.pop()
            };
            
            if let Some(task) = download_task {
                let url = task.url.clone();
                let depth = task.depth;
                let priority = task.priority.clone();
                let resource_type = task.resource_type.clone();
                // Check depth limit (0 means unlimited)
                if self.max_depth > 0 && depth > self.max_depth {
                    continue;
                }
                
                let client = self.client.clone();
                let file_manager = self.file_manager.clone();
                let html_parser = self.html_parser.clone();
                let visited_urls = self.visited_urls.clone();
                let download_queue = self.download_queue.clone();
                let download_cache = self.download_cache.clone();
                
                progress_bar.set_message(format!("Downloading: {}", url));
                
                let base_url = self.base_url.clone();
                let download_external = self.download_external;
                
                // Process the download directly instead of spawning a task
                println!("🚀 Processing download for: {}", url);
                                        if let Err(e) = Self::download_and_process_url(
                            &client,
                            &file_manager,
                            &html_parser,
                            &url,
                            depth,
                            &visited_urls,
                            &download_queue,
                            &download_cache,
                            download_external,
                            &base_url,
                            priority,
                            resource_type,
                            &self.only_resources,
                        ).await {
                    eprintln!("❌ Error downloading {}: {}", url, e);
                }
                println!("🏁 Download completed for: {}", url);
            } else {
                // Check if all downloads are complete
                let queue_size = self.download_queue.lock().unwrap().len();
                
                if queue_size == 0 {
                    // Wait a bit for any ongoing downloads to complete
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    
                    let final_queue_size = self.download_queue.lock().unwrap().len();
                    if final_queue_size == 0 {
                        break;
                    }
                }
                
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
        
        progress_bar.finish_with_message("✅ All downloads completed!");
        
        let visited_count = self.visited_urls.lock().unwrap().len();
        println!("📊 Total pages downloaded: {}", visited_count);
        
        Ok(())
    }
    
    async fn download_and_process_url(
        client: &Client,
        file_manager: &FileManager,
        html_parser: &HtmlParser,
        url: &str,
        depth: usize,
        visited_urls: &Arc<Mutex<HashSet<String>>>,
        download_queue: &Arc<Mutex<BinaryHeap<DownloadTask>>>,
        download_cache: &Arc<Mutex<HashMap<String, String>>>,
        download_external: bool,
        base_url: &str,
        priority: DownloadPriority,
        resource_type: Option<ResourceType>,
        only_resources: &Option<Vec<String>>,
    ) -> Result<()> {
        // Check if already visited
        {
            let mut visited = visited_urls.lock().unwrap();
            if visited.contains(url) {
                return Ok(());
            }
            visited.insert(url.to_string());
        }
        
        let priority_str = match priority {
            DownloadPriority::Critical => "🔥 CRITICAL",
            DownloadPriority::High => "⚡ HIGH",
            DownloadPriority::Normal => "📥 NORMAL",
        };
        println!("{} Downloading: {} (depth: {})", priority_str, url, depth);
        
        // Download the URL
        println!("🌐 Sending request to: {}", url);
        let response = match client.get(url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                eprintln!("❌ Request failed: {}", e);
                return Ok(());
            }
        };
        
        println!("📡 Response status: {}", response.status());
        
        if response.status() != StatusCode::OK {
            eprintln!("⚠️  HTTP {} for {}", response.status(), url);
            return Ok(());
        }
        
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html")
            .to_string();
        
        let content = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("❌ Failed to read response body: {}", e);
                return Ok(());
            }
        };
        
        // Determine content type
        let is_html = content_type.contains("text/html") || 
                     content.starts_with(b"<!DOCTYPE") || 
                     content.starts_with(b"<html");
        let is_css = content_type.contains("text/css") || url.ends_with(".css");
        
        println!("🔍 Content type: {}, is_html: {}, is_css: {}", content_type, is_html, is_css);
        println!("🔍 Content preview: {}", String::from_utf8_lossy(&content[..content.len().min(100)]));
        
        if is_html {
            // Parse HTML and extract resources
            let html_content = String::from_utf8_lossy(&content);
            
            // Create a new HTML parser with the current page's base URL
            let page_html_parser = HtmlParser::new(url)?;
            let resources = page_html_parser.extract_resources(&html_content)?;
            
            // Helper function to check if a resource type should be processed
            let should_process_resource_type = |resource_type: &ResourceType| -> bool {
                if let Some(ref only_resources) = only_resources {
                    let type_str = match resource_type {
                        ResourceType::Image => "images",
                        ResourceType::CSS => "css",
                        ResourceType::JavaScript => "js",
                        ResourceType::Link => "html",
                        ResourceType::Other => "other",
                    };
                    only_resources.iter().any(|r| r.to_lowercase() == type_str)
                } else {
                    // If no filter specified, process all resource types
                    true
                }
            };
            
            // Process resources in priority order: CSS/JS first, then HTML, then images
            let mut critical_resources = Vec::new();
            let mut high_resources = Vec::new();
            let mut normal_resources = Vec::new();
            
            // Categorize resources by priority
            for resource in &resources {
                let priority = match resource.resource_type {
                    ResourceType::CSS | ResourceType::JavaScript => DownloadPriority::Critical,
                    ResourceType::Link => DownloadPriority::High,
                    ResourceType::Image | ResourceType::Other => DownloadPriority::Normal,
                };
                
                let should_download = match resource.resource_type {
                    ResourceType::Image | ResourceType::CSS | ResourceType::JavaScript => {
                        // Always download media files (images, CSS, JS) from any site
                        // But respect the only_resources filter
                        should_process_resource_type(&resource.resource_type)
                    },
                    ResourceType::Link => {
                        // Only download HTML pages from the target site
                        // And respect the only_resources filter
                        resource.original_url.contains(base_url) && should_process_resource_type(&resource.resource_type)
                    },
                    ResourceType::Other => {
                        // Download other resources only from target site
                        // And respect the only_resources filter
                        resource.original_url.contains(base_url) && should_process_resource_type(&resource.resource_type)
                    }
                };
                
                if should_download {
                    match priority {
                        DownloadPriority::Critical => critical_resources.push(resource.clone()),
                        DownloadPriority::High => high_resources.push(resource.clone()),
                        DownloadPriority::Normal => normal_resources.push(resource.clone()),
                    }
                } else if !resource.original_url.contains(base_url) {
                    // Log when we skip external HTML pages
                    match resource.resource_type {
                        ResourceType::Link => println!("⏭️  Skipping external page: {} (but will download its media)", resource.original_url),
                        _ => {}
                    }
                } else if !should_process_resource_type(&resource.resource_type) {
                    // Log when we skip resources due to filter
                    let resource_type_str = match resource.resource_type {
                        ResourceType::Image => "Image",
                        ResourceType::CSS => "CSS",
                        ResourceType::JavaScript => "JavaScript",
                        ResourceType::Link => "Link",
                        ResourceType::Other => "Other",
                    };
                    println!("🔍 Skipping {} due to resource filter: {}", resource_type_str, resource.original_url);
                }
            }
            
            // Download critical resources first (CSS/JS) and collect local paths for HTML rewriting
            let mut html_content_updated = html_content.to_string();
            for resource in &critical_resources {
                let resource_type_str = match resource.resource_type {
                    ResourceType::CSS => "CSS",
                    ResourceType::JavaScript => "JavaScript",
                    _ => "Critical",
                };
                println!("🔥 Processing CRITICAL {} resource: {}", resource_type_str, resource.original_url);
                
                if let Err(e) = Self::download_resource(
                    client,
                    file_manager,
                    &page_html_parser,
                    &resource.original_url,
                    download_cache,
                ).await {
                    eprintln!("⚠️  Failed to download CRITICAL {} resource {}: {}", resource_type_str, resource.original_url, e);
                } else {
                    // Get the local path for this resource and update HTML content
                    if let Ok(local_path) = page_html_parser.url_to_local_path_string(&resource.original_url) {
                        html_content_updated = html_content_updated.replace(&resource.original_url, &local_path);
                        println!("🔄 Updated HTML: {} -> {}", resource.original_url, local_path);
                    }
                }
            }
            
            // Add high priority resources (HTML pages) to queue
            for resource in &high_resources {
                if !visited_urls.lock().unwrap().contains(&resource.original_url) {
                    let mut queue = download_queue.lock().unwrap();
                    queue.push(DownloadTask {
                        url: resource.original_url.clone(),
                        depth: depth + 1,
                        priority: DownloadPriority::High,
                        resource_type: Some(resource.resource_type.clone()),
                    });
                    println!("⚡ Queued HIGH priority HTML page: {}", resource.original_url);
                }
            }
            
            // Download normal priority resources (images, etc.) and update HTML content
            for resource in &normal_resources {
                let resource_type_str = match resource.resource_type {
                    ResourceType::Image => "Image",
                    ResourceType::Other => "Other",
                    _ => "Normal",
                };
                println!("📥 Processing NORMAL {} resource: {}", resource_type_str, resource.original_url);
                
                if let Err(e) = Self::download_resource(
                    client,
                    file_manager,
                    &page_html_parser,
                    &resource.original_url,
                    download_cache,
                ).await {
                    eprintln!("⚠️  Failed to download NORMAL {} resource {}: {}", resource_type_str, resource.original_url, e);
                } else {
                    // Get the local path for this resource and update HTML content
                    if let Ok(local_path) = page_html_parser.url_to_local_path_string(&resource.original_url) {
                        html_content_updated = html_content_updated.replace(&resource.original_url, &local_path);
                        println!("🔄 Updated HTML: {} -> {}", resource.original_url, local_path);
                    }
                }
            }
            
            // Save the updated HTML with local paths for resources
            let local_path = page_html_parser.url_to_local_path_string(url)?;
            println!("💾 Saving HTML to: {}", local_path);
            let saved_path = file_manager.save_file(&local_path, html_content_updated.as_bytes(), Some(&content_type))?;
            println!("✅ Saved HTML to: {}", saved_path.display());
            
            // Note: Links are now processed in the priority-based resource processing above
            // This section is no longer needed as links are queued with proper priority
        } else if is_css {
            // Process CSS files to extract background images
            let css_content = String::from_utf8_lossy(&content);
            let page_html_parser = HtmlParser::new(url)?;
            
            // Extract background images from CSS
            let mut background_resources = Vec::new();
            page_html_parser.extract_background_images_from_css(&css_content, &mut background_resources);
            
                        // Download background images with normal priority (after CSS/JS)
            for resource in &background_resources {
                // Always download background images from any site
                // This ensures the CSS renders without 404 errors
                println!("📥 Processing NORMAL background image: {}", resource.original_url);
                if let Err(e) = Self::download_resource(
                    client,
                    file_manager,
                    &page_html_parser,
                    &resource.original_url,
                    download_cache,
                ).await {
                    eprintln!("⚠️  Failed to download background image {}: {}", resource.original_url, e);
                }
            }
            
            // Save the CSS file
            let local_path = page_html_parser.url_to_local_path_string(url)?;
            println!("💾 Saving CSS to: {}", local_path);
            let saved_path = file_manager.save_file(&local_path, &content, Some(&content_type))?;
            println!("✅ Saved CSS to: {:?}", saved_path);
        } else {
            // Save non-HTML content as-is
            let local_path = html_parser.url_to_local_path_string(url)?;
            println!("💾 Saving non-HTML to: {}", local_path);
            let saved_path = file_manager.save_file(&local_path, &content, Some(&content_type))?;
            println!("✅ Saved non-HTML to: {:?}", saved_path);
        }
        
        println!("✅ Downloaded: {}", url);
        Ok(())
    }
    
    async fn download_resource(
        client: &Client,
        file_manager: &FileManager,
        html_parser: &HtmlParser,
        url: &str,
        download_cache: &Arc<Mutex<HashMap<String, String>>>,
    ) -> Result<()> {
        // Check if already downloaded using cache
        {
            let cache = download_cache.lock().unwrap();
            if cache.contains_key(url) {
                let cached_path = cache.get(url).unwrap();
                println!("⏭️  Skipping {} (already downloaded to {})", url, cached_path);
                return Ok(());
            }
        }
        
        // Check if file exists on disk
        if file_manager.file_exists(url) {
            // Add to cache for future reference
            let local_path = html_parser.url_to_local_path_string(url)?;
            let mut cache = download_cache.lock().unwrap();
            cache.insert(url.to_string(), local_path.clone());
            println!("⏭️  Skipping {} (already exists on disk)", url);
            return Ok(());
        }
        
        // Determine resource type for better logging
        let resource_type = if url.ends_with(".css") || url.contains("/css/") {
            "CSS"
        } else if url.ends_with(".js") || url.contains("/js/") {
            "JavaScript"
        } else if url.ends_with(".png") || url.ends_with(".jpg") || url.ends_with(".jpeg") || 
                  url.ends_with(".gif") || url.ends_with(".webp") || url.ends_with(".svg") {
            "Image"
        } else if url.ends_with(".woff") || url.ends_with(".woff2") || url.ends_with(".ttf") || 
                  url.ends_with(".eot") {
            "Font"
        } else {
            "Resource"
        };
        
        println!("📥 Downloading {}: {}", resource_type, url);
        
        let response = match client.get(url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                eprintln!("❌ Failed to send request for {} {}: {}", resource_type, url, e);
                return Ok(());
            }
        };
        
        if response.status() != StatusCode::OK {
            eprintln!("⚠️  HTTP {} for {} {}", response.status(), resource_type, url);
            return Ok(());
        }
        
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        
        let content = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("❌ Failed to read {} body {}: {}", resource_type, url, e);
                return Ok(());
            }
        };
        
        // Save the resource
        let local_path = match html_parser.url_to_local_path_string(url) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("❌ Failed to convert URL to local path {}: {}", url, e);
                return Ok(());
            }
        };
        
        let saved_path = match file_manager.save_file(&local_path, &content, Some(&content_type)) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("❌ Failed to save {} {}: {}", resource_type, url, e);
                return Ok(());
            }
        };
        
        // Add to download cache
        {
            let mut cache = download_cache.lock().unwrap();
            cache.insert(url.to_string(), local_path.clone());
        }
        
        println!("✅ Downloaded {} to: {}", resource_type, saved_path.display());
        
        Ok(())
    }
} 
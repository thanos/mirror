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
use webp::Encoder;

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

#[derive(Clone, Debug)]
pub struct WebsiteMirror {
    base_url: String,
    output_dir: PathBuf,
    max_depth: usize,
    max_concurrent: usize,
    ignore_robots: bool,
    download_external: bool,
    only_resources: Option<Vec<String>>,
    convert_to_webp: bool,
    client: Client,
    file_manager: FileManager,
    html_parser: HtmlParser,
    visited_urls: Arc<Mutex<HashSet<String>>>,
    download_queue: Arc<Mutex<BinaryHeap<DownloadTask>>>,
    semaphore: Arc<Semaphore>,
    download_cache: Arc<Mutex<HashMap<String, String>>>, // URL -> local path mapping
}

impl WebsiteMirror {
    /// Get the local path for a resource, converting image extensions to WebP if needed
    fn get_local_path_for_resource(&self, html_parser: &HtmlParser, original_url: &str) -> Result<String> {
        let local_path = html_parser.url_to_local_path_string(original_url)?;
        
        // Convert image extensions to WebP for JPEG/PNG files if the flag is enabled
        if self.convert_to_webp && (original_url.ends_with(".jpg") || original_url.ends_with(".jpeg") || original_url.ends_with(".png")) {
            let webp_path = local_path.replace(".jpg", ".webp")
                                    .replace(".jpeg", ".webp")
                                    .replace(".png", ".webp");
            Ok(webp_path)
        } else {
            Ok(local_path)
        }
    }

    /// Static version for use in functions without self access
    fn get_local_path_for_resource_static(html_parser: &HtmlParser, original_url: &str, convert_to_webp: bool, current_html_path: &str) -> Result<String> {
        let local_path = html_parser.url_to_local_path_string(original_url)?;
        
        // Convert image extensions to WebP for JPEG/PNG files if the flag is enabled
        let final_local_path = if convert_to_webp && (original_url.ends_with(".jpg") || original_url.ends_with(".jpeg") || original_url.ends_with(".png")) {
            local_path.replace(".jpg", ".webp")
                     .replace(".jpeg", ".webp")
                     .replace(".png", ".webp")
        } else {
            local_path
        };
        
        // Calculate relative path from current HTML file to the resource
        let relative_path = Self::calculate_relative_path(current_html_path, &final_local_path);
        Ok(relative_path)
    }

    /// Calculate relative path from source file to target file
    fn calculate_relative_path(from_path: &str, to_path: &str) -> String {
        use std::path::Path;
        
        let from = Path::new(from_path);
        let to = Path::new(to_path);
        
        // Get the directory of the source file
        let from_dir = from.parent().unwrap_or(Path::new(""));
        
        // Calculate relative path
        match pathdiff::diff_paths(to, from_dir) {
            Some(relative) => relative.to_string_lossy().to_string(),
            None => to_path.to_string(), // Fallback to absolute path
        }
    }

    /// Convert JPEG/PNG images to WebP format with good quality lossy compression
    fn convert_to_webp(&self, image_data: &[u8], original_url: &str) -> Result<Vec<u8>> {
        Self::convert_to_webp_static(image_data, original_url)
    }

    /// Static version for use in functions without self access
    fn convert_to_webp_static(image_data: &[u8], original_url: &str) -> Result<Vec<u8>> {
        // Decode the image
        let img = match image::load_from_memory(image_data) {
            Ok(img) => img,
            Err(e) => {
                eprintln!("⚠️  Failed to decode image {}: {}", original_url, e);
                return Ok(image_data.to_vec()); // Return original data if conversion fails
            }
        };
        
        // Convert to RGB8 if needed (WebP encoder expects RGB)
        let rgb_img = img.to_rgb8();
        
        // Create WebP encoder with good quality (80/100)
        let encoder = Encoder::from_rgb(&rgb_img, rgb_img.width(), rgb_img.height());
        
        // Encode with quality 80 (good balance between size and quality)
        let webp_data = encoder.encode(80.0);
        
        let original_size = image_data.len();
        let webp_size = webp_data.len();
        let compression_ratio = (original_size as f64 / webp_size as f64 * 100.0) as u32;
        
        println!("🔄 Converted {} to WebP: {} -> {} bytes ({}% of original size)", 
                 original_url, original_size, webp_size, compression_ratio);
        
        Ok(webp_data.to_vec())
    }

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
        convert_to_webp: bool,
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
            convert_to_webp,
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
                            self.convert_to_webp,
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
        convert_to_webp: bool,
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
            
            // Calculate the local path for the current HTML file (needed for relative path calculations)
            let current_html_path = page_html_parser.url_to_local_path_string(url)?;
            
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
                    convert_to_webp,
                ).await {
                    eprintln!("⚠️  Failed to download CRITICAL {} resource {}: {}", resource_type_str, resource.original_url, e);
                } else {
                    // Get the local path for this resource and update HTML content
                    if let Ok(local_path) = Self::get_local_path_for_resource_static(&page_html_parser, &resource.original_url, convert_to_webp, &current_html_path) {
                        let before_count = html_content_updated.matches(&resource.original_url).count();
                        html_content_updated = html_content_updated.replace(&resource.original_url, &local_path);
                        let after_count = html_content_updated.matches(&local_path).count();
                        println!("🔄 Updated HTML: {} -> {} ({} replacements)", resource.original_url, local_path, after_count);
                        
                        // Debug: Check if the replacement actually worked
                        if before_count > 0 && after_count == 0 {
                            eprintln!("⚠️  Warning: URL replacement may have failed for: {}", resource.original_url);
                        }
                        
                        // If this is a WebP conversion, also update any remaining references to the old extension
                        if convert_to_webp && (resource.original_url.ends_with(".jpg") || resource.original_url.ends_with(".jpeg") || resource.original_url.ends_with(".png")) {
                            let old_extension = if resource.original_url.ends_with(".jpg") {
                                ".jpg"
                            } else if resource.original_url.ends_with(".jpeg") {
                                ".jpeg"
                            } else {
                                ".png"
                            };
                            
                            // Extract just the filename part for extension replacement
                            if let Some(filename) = resource.original_url.split('/').last() {
                                let new_filename = filename.replace(old_extension, ".webp");
                                let old_filename_with_path = resource.original_url;
                                let new_filename_with_path = resource.original_url.replace(filename, &new_filename);
                                
                                // Replace the filename with .webp extension
                                let before_ext_count = html_content_updated.matches(&old_filename_with_path).count();
                                html_content_updated = html_content_updated.replace(&old_filename_with_path, &new_filename_with_path);
                                let after_ext_count = html_content_updated.matches(&new_filename_with_path).count();
                                
                                if before_ext_count > 0 {
                                    println!("🔄 Updated file extension: {} -> {} ({} replacements)", 
                                             old_filename_with_path, new_filename_with_path, after_ext_count);
                                }
                            }
                        }
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
                    convert_to_webp,
                ).await {
                    eprintln!("⚠️  Failed to download NORMAL {} resource {}: {}", resource_type_str, resource.original_url, e);
                } else {
                    // Get the local path for this resource and update HTML content
                    if let Ok(local_path) = Self::get_local_path_for_resource_static(&page_html_parser, &resource.original_url, convert_to_webp, &current_html_path) {
                        let before_count = html_content_updated.matches(&resource.original_url).count();
                        html_content_updated = html_content_updated.replace(&resource.original_url, &local_path);
                        let after_count = html_content_updated.matches(&local_path).count();
                        println!("🔄 Updated HTML: {} -> {} ({} replacements)", resource.original_url, local_path, after_count);
                        
                        // Debug: Check if the replacement actually worked
                        if before_count > 0 && after_count == 0 {
                            eprintln!("⚠️  Warning: URL replacement may have failed for: {}", resource.original_url);
                        }
                        
                        // If this is a WebP conversion, also update any remaining references to the old extension
                        if convert_to_webp && (resource.original_url.ends_with(".jpg") || resource.original_url.ends_with(".jpeg") || resource.original_url.ends_with(".png")) {
                            let old_extension = if resource.original_url.ends_with(".jpg") {
                                ".jpg"
                            } else if resource.original_url.ends_with(".jpeg") {
                                ".jpeg"
                            } else {
                                ".png"
                            };
                            
                            // Extract just the filename part for extension replacement
                            if let Some(filename) = resource.original_url.split('/').last() {
                                let new_filename = filename.replace(old_extension, ".webp");
                                let old_filename_with_path = resource.original_url;
                                let new_filename_with_path = resource.original_url.replace(filename, &new_filename);
                                
                                // Replace the filename with .webp extension
                                let before_ext_count = html_content_updated.matches(&old_filename_with_path).count();
                                html_content_updated = html_content_updated.replace(&old_filename_with_path, &new_filename_with_path);
                                let after_ext_count = html_content_updated.matches(&new_filename_with_path).count();
                                
                                if before_ext_count > 0 {
                                    println!("🔄 Updated file extension: {} -> {} ({} replacements)", 
                                             old_filename_with_path, new_filename_with_path, after_ext_count);
                                }
                            }
                        }
                    }
                }
            }
            
            // Debug: Show a preview of the updated HTML content
            println!("🔍 HTML content preview (first 500 chars):");
            let preview = html_content_updated.chars().take(500).collect::<String>();
            println!("{}", preview);
            
            // Save the updated HTML with local paths for resources
            println!("💾 Saving HTML to: {}", current_html_path);
            let saved_path = file_manager.save_file(&current_html_path, html_content_updated.as_bytes(), Some(&content_type))?;
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
                    convert_to_webp,
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
        convert_to_webp: bool,
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
        
        // Convert images to WebP if they're JPEG or PNG and the flag is enabled
        let (final_content, final_content_type, final_local_path) = if convert_to_webp && (url.ends_with(".jpg") || url.ends_with(".jpeg") || url.ends_with(".png")) {
            // Convert to WebP
            let webp_data = Self::convert_to_webp_static(&content, url)?;
            
            // Change file extension to .webp
            let webp_path = local_path.replace(".jpg", ".webp")
                                    .replace(".jpeg", ".webp")
                                    .replace(".png", ".webp");
            
            (webp_data, "image/webp".to_string(), webp_path)
        } else {
            // Keep original content and path
            (content.to_vec(), content_type, local_path)
        };
        
        let saved_path = match file_manager.save_file(&final_local_path, &final_content, Some(&final_content_type)) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("❌ Failed to save {} {}: {}", resource_type, url, e);
                return Ok(());
            }
        };
        
        // Add to download cache
        {
            let mut cache = download_cache.lock().unwrap();
            cache.insert(url.to_string(), final_local_path.to_string());
        }
        
        println!("✅ Downloaded {} to: {}", resource_type, saved_path.display());
        
        Ok(())
    }
} 

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::sync::Arc;
    use std::collections::HashMap;

    #[test]
    fn test_website_mirror_new() {
        let temp_dir = tempdir().unwrap();
        let mirror = WebsiteMirror::new(
            "https://example.com",
            temp_dir.path(),
            3,
            10,
            false,
            false,
            None,
            false
        ).unwrap();
        
        assert_eq!(mirror.base_url.as_str(), "https://example.com");
        assert_eq!(mirror.max_depth, 3);
        assert_eq!(mirror.max_concurrent, 10);
        assert_eq!(mirror.ignore_robots, false);
        assert_eq!(mirror.download_external, false);
        assert_eq!(mirror.convert_to_webp, false);
    }

    #[test]
    fn test_website_mirror_new_with_options() {
        let temp_dir = tempdir().unwrap();
        let mirror = WebsiteMirror::new(
            "https://example.com",
            temp_dir.path(),
            5,
            20,
            true,
            true,
            Some(vec!["images".to_string()]),
            true
        ).unwrap();
        
        assert_eq!(mirror.max_depth, 5);
        assert_eq!(mirror.max_concurrent, 20);
        assert_eq!(mirror.ignore_robots, true);
        assert_eq!(mirror.download_external, true);
        assert_eq!(mirror.convert_to_webp, true);
        assert_eq!(mirror.only_resources, Some(vec!["images".to_string()]));
    }

    #[test]
    fn test_should_process_resource_type() {
        let temp_dir = tempdir().unwrap();
        let mirror = WebsiteMirror::new(
            "https://example.com",
            temp_dir.path(),
            3,
            10,
            false,
            false,
            None,
            false
        ).unwrap();
        
        // Test with no restrictions
        assert!(mirror.should_process_resource_type(&ResourceType::CSS));
        assert!(mirror.should_process_resource_type(&ResourceType::JavaScript));
        assert!(mirror.should_process_resource_type(&ResourceType::Image));
        assert!(mirror.should_process_resource_type(&ResourceType::Link));
        
        // Test with specific restrictions
        let mirror = WebsiteMirror::new(
            "https://example.com",
            temp_dir.path(),
            3,
            10,
            false,
            false,
            Some(vec!["images".to_string(), "css".to_string()]),
            false
        ).unwrap();
        
        assert!(mirror.should_process_resource_type(&ResourceType::CSS));
        assert!(!mirror.should_process_resource_type(&ResourceType::JavaScript));
        assert!(mirror.should_process_resource_type(&ResourceType::Image));
        assert!(!mirror.should_process_resource_type(&ResourceType::Link));
    }

    #[test]
    fn test_convert_to_webp_success() {
        // Create a simple test image (1x1 pixel PNG)
        let png_data = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
            0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00,
            0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0x99, 0x01, 0x01, 0x00, 0x00, 0x00,
            0xFF, 0xFF, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC, 0x33,
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82
        ];
        
        let result = WebsiteMirror::convert_to_webp_static(&png_data, "test.png");
        assert!(result.is_ok());
        
        let webp_data = result.unwrap();
        assert!(webp_data.len() > 0);
        assert!(webp_data.len() != png_data.len()); // Should be different size
    }

    #[test]
    fn test_convert_to_webp_invalid_image() {
        let invalid_data = b"not an image";
        let result = WebsiteMirror::convert_to_webp_static(invalid_data, "test.txt");
        assert!(result.is_ok()); // Should return original data on failure
        
        let returned_data = result.unwrap();
        assert_eq!(returned_data, invalid_data);
    }

    #[test]
    fn test_get_local_path_for_resource_static() {
        let html_parser = HtmlParser::new("https://example.com").unwrap();
        
        // Test normal path
        let result = WebsiteMirror::get_local_path_for_resource_static(
            &html_parser, 
            "https://example.com/image.jpg", 
            false,
            "index.html"
        ).unwrap();
        assert_eq!(result, "image.jpg");
        
        // Test WebP conversion
                  let result = WebsiteMirror::get_local_path_for_resource_static(
            &html_parser, 
            "https://example.com/image.jpg", 
            true,
            "index.html"
        ).unwrap();
        assert_eq!(result, "image.webp");
        
        // Test PNG conversion
                  let result = WebsiteMirror::get_local_path_for_resource_static(
            &html_parser, 
            "https://example.com/image.png", 
            true,
            "index.html"
        ).unwrap();
        assert_eq!(result, "image.webp");
        
        // Test non-image file
                  let result = WebsiteMirror::get_local_path_for_resource_static(
            &html_parser, 
            "https://example.com/style.css", 
            true,
            "index.html"
        ).unwrap();
        assert_eq!(result, "style.css");
    }

    #[test]
    fn test_download_task_ordering() {
        let task1 = DownloadTask {
            url: "https://example.com/style.css".to_string(),
            depth: 1,
            priority: DownloadPriority::Critical,
            resource_type: Some(ResourceType::CSS),
        };
        
        let task2 = DownloadTask {
            url: "https://example.com/page.html".to_string(),
            depth: 1,
            priority: DownloadPriority::High,
            resource_type: Some(ResourceType::Link),
        };
        
        let task3 = DownloadTask {
            url: "https://example.com/image.jpg".to_string(),
            depth: 1,
            priority: DownloadPriority::Normal,
            resource_type: Some(ResourceType::Image),
        };
        
        let task4 = DownloadTask {
            url: "https://example.com/script.js".to_string(),
            depth: 2,
            priority: DownloadPriority::Critical,
            resource_type: Some(ResourceType::JavaScript),
        };
        
        // Critical should come before High
        assert!(task1 > task2);
        
        // High should come before Normal
        assert!(task2 > task3);
        
        // Same priority, lower depth should come first
        assert!(task1 > task4);
        
        // Test PartialOrd
        assert!(task1 >= task2);
        assert!(task2 <= task1);
    }

    #[test]
    fn test_download_task_equality() {
        let task1 = DownloadTask {
            url: "https://example.com/style.css".to_string(),
            depth: 1,
            priority: DownloadPriority::Critical,
            resource_type: Some(ResourceType::CSS),
        };
        
        let task2 = DownloadTask {
            url: "https://example.com/style.css".to_string(),
            depth: 1,
            priority: DownloadPriority::Critical,
            resource_type: Some(ResourceType::CSS),
        };
        
        let task3 = DownloadTask {
            url: "https://example.com/script.js".to_string(),
            depth: 1,
            priority: DownloadPriority::Critical,
            resource_type: Some(ResourceType::JavaScript),
        };
        
        assert_eq!(task1, task2);
        assert_ne!(task1, task3);
    }

    #[test]
    fn test_download_priority_ordering() {
        assert!(DownloadPriority::Critical > DownloadPriority::High);
        assert!(DownloadPriority::High > DownloadPriority::Normal);
        // Normal is the lowest priority we have
    }

    #[test]
    fn test_download_priority_debug() {
        assert_eq!(format!("{:?}", DownloadPriority::Critical), "Critical");
        assert_eq!(format!("{:?}", DownloadPriority::High), "High");
        assert_eq!(format!("{:?}", DownloadPriority::Normal), "Normal");
        // We only have 3 priority levels
    }

    #[test]
    fn test_download_priority_clone() {
        let priority = DownloadPriority::Critical;
        let cloned = priority.clone();
        assert_eq!(cloned, priority);
    }

    #[test]
    fn test_website_mirror_debug() {
        let temp_dir = tempdir().unwrap();
        let mirror = WebsiteMirror::new(
            "https://example.com",
            temp_dir.path(),
            3,
            10,
            false,
            false,
            None,
            false
        ).unwrap();
        
        let debug_str = format!("{:?}", mirror);
        assert!(debug_str.contains("WebsiteMirror"));
        assert!(debug_str.contains("example.com"));
    }

    #[test]
    fn test_website_mirror_clone() {
        let temp_dir = tempdir().unwrap();
        let mirror = WebsiteMirror::new(
            "https://example.com",
            temp_dir.path(),
            3,
            10,
            false,
            false,
            None,
            false
        ).unwrap();
        
        let cloned = mirror.clone();
        assert_eq!(mirror.base_url, cloned.base_url);
        assert_eq!(mirror.max_depth, cloned.max_depth);
        assert_eq!(mirror.max_concurrent, cloned.max_concurrent);
        assert_eq!(mirror.ignore_robots, cloned.ignore_robots);
        assert_eq!(mirror.download_external, cloned.download_external);
        assert_eq!(mirror.convert_to_webp, cloned.convert_to_webp);
    }

    // Note: WebsiteMirror doesn't implement PartialEq, Eq, or Hash due to complex fields
    // These tests are removed as they're not essential for functionality

    /// Test that WebP extension rewriting works correctly in HTML content
    #[test]
    fn test_webp_extension_rewriting_in_html() {
        let temp_dir = tempdir().unwrap();
        let html_parser = HtmlParser::new("https://example.com").unwrap();
        
        // Test HTML content with image references
        let mut html_content = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <img src="https://example.com/photo.jpg" alt="Photo">
                <img src="https://example.com/logo.png" alt="Logo">
                <img src="https://example.com/banner.jpeg" alt="Banner">
            </body>
            </html>
        "#.to_string();
        
        // Simulate the HTML rewriting process for WebP conversion
        let test_urls = vec![
            "https://example.com/photo.jpg",
            "https://example.com/logo.png",
            "https://example.com/banner.jpeg",
        ];
        
        for url in test_urls {
            // Get local path with WebP conversion
            let local_path = WebsiteMirror::get_local_path_for_resource_static(
                &html_parser,
                url,
                true, // convert_to_webp = true
                "index.html"
            ).unwrap();
            
            // Replace the original URL with the local path
            html_content = html_content.replace(url, &local_path);
            
            // Handle WebP extension replacement
            if url.ends_with(".jpg") || url.ends_with(".jpeg") || url.ends_with(".png") {
                let old_extension = if url.ends_with(".jpg") {
                    ".jpg"
                } else if url.ends_with(".jpeg") {
                    ".jpeg"
                } else {
                    ".png"
                };
                
                // Extract filename for extension replacement
                if let Some(filename) = url.split('/').last() {
                    let new_filename = filename.replace(old_extension, ".webp");
                    let old_filename_with_path = url;
                    let new_filename_with_path = url.replace(filename, &new_filename);
                    
                    // Replace the filename with .webp extension
                    html_content = html_content.replace(&old_filename_with_path, &new_filename_with_path);
                }
            }
        }
        
        // Verify that all image references now use .webp extensions
        assert!(html_content.contains("photo.webp"), "Should contain photo.webp");
        assert!(html_content.contains("logo.webp"), "Should contain logo.webp");
        assert!(html_content.contains("banner.webp"), "Should contain banner.webp");
        
        // Verify that original extensions are no longer present
        assert!(!html_content.contains(".jpg"), "Should not contain .jpg");
        assert!(!html_content.contains(".jpeg"), "Should not contain .jpeg");
        assert!(!html_content.contains(".png"), "Should not contain .png");
        
        println!("Updated HTML content:");
        println!("{}", html_content);
    }
} 
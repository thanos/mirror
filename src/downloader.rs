use anyhow::Result;
use reqwest::{Client, ClientBuilder, StatusCode};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use std::path::{Path, PathBuf};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};

use crate::file_manager::FileManager;
use crate::html_parser::{HtmlParser, ResourceType};

pub struct WebsiteMirror {
    base_url: String,
    output_dir: PathBuf,
    max_depth: usize,
    max_concurrent: usize,
    ignore_robots: bool,
    download_external: bool,
    client: Client,
    file_manager: FileManager,
    html_parser: HtmlParser,
    visited_urls: Arc<Mutex<HashSet<String>>>,
    download_queue: Arc<Mutex<VecDeque<(String, usize)>>>,
    semaphore: Arc<Semaphore>,
}

impl WebsiteMirror {
    pub fn new(
        base_url: &str,
        output_dir: &Path,
        max_depth: usize,
        max_concurrent: usize,
        ignore_robots: bool,
        download_external: bool,
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
            client,
            file_manager,
            html_parser,
            visited_urls: Arc::new(Mutex::new(HashSet::new())),
            download_queue: Arc::new(Mutex::new(VecDeque::new())),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
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
        
        // Add the base URL to the download queue
        {
            let mut queue = self.download_queue.lock().unwrap();
            queue.push_back((self.base_url.clone(), 0));
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
                queue.pop_front()
            };
            
            if let Some((url, depth)) = download_task {
                // Check depth limit (0 means unlimited)
                if self.max_depth > 0 && depth > self.max_depth {
                    continue;
                }
                
                let client = self.client.clone();
                let file_manager = self.file_manager.clone();
                let html_parser = self.html_parser.clone();
                let visited_urls = self.visited_urls.clone();
                let download_queue = self.download_queue.clone();
                
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
                    download_external,
                    &base_url,
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
        download_queue: &Arc<Mutex<VecDeque<(String, usize)>>>,
        download_external: bool,
        base_url: &str,
    ) -> Result<()> {
        // Check if already visited
        {
            let mut visited = visited_urls.lock().unwrap();
            if visited.contains(url) {
                return Ok(());
            }
            visited.insert(url.to_string());
        }
        
        println!("📥 Downloading: {} (depth: {})", url, depth);
        
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
            
            // Download ALL resources needed by this page to render properly
            for resource in &resources {
                let should_download = match resource.resource_type {
                    ResourceType::Image | ResourceType::CSS | ResourceType::JavaScript => {
                        // Always download media files (images, CSS, JS) from any site
                        // This ensures the page renders without 404 errors
                        true
                    },
                    ResourceType::Link => {
                        // Only download HTML pages from the target site
                        resource.original_url.contains(base_url)
                    },
                    ResourceType::Other => {
                        // Download other resources only from target site
                        resource.original_url.contains(base_url)
                    }
                };
                
                if should_download {
                    if let Err(e) = Self::download_resource(
                        client,
                        file_manager,
                        &page_html_parser,
                        &resource.original_url,
                    ).await {
                        eprintln!("⚠️  Failed to download resource {}: {}", resource.original_url, e);
                    }
                } else if !resource.original_url.contains(base_url) {
                    // Log when we skip external HTML pages
                    match resource.resource_type {
                        ResourceType::Link => println!("⏭️  Skipping external page: {} (but will download its media)", resource.original_url),
                        _ => {}
                    }
                }
            }
            
            // Convert HTML links to local paths
            let modified_html = html_parser.convert_html_links(&html_content)?;
            let modified_content = modified_html.as_bytes();
            
            // Save the modified HTML
            let local_path = page_html_parser.url_to_local_path_string(url)?;
            println!("💾 Saving HTML to: {}", local_path);
            let saved_path = file_manager.save_file(&local_path, modified_content, Some(&content_type))?;
            println!("✅ Saved HTML to: {:?}", saved_path);
            
            // Add new links to the download queue
            for resource in &resources {
                if resource.resource_type == ResourceType::Link {
                    // Only crawl pages from the same domain as the target site
                    if resource.original_url.contains(base_url) {
                        let mut queue = download_queue.lock().unwrap();
                        if !visited_urls.lock().unwrap().contains(&resource.original_url) {
                            queue.push_back((resource.original_url.clone(), depth + 1));
                        }
                    }
                    // External links are not crawled, just their resources are downloaded
                }
            }
        } else if is_css {
            // Process CSS files to extract background images
            let css_content = String::from_utf8_lossy(&content);
            let page_html_parser = HtmlParser::new(url)?;
            
            // Extract background images from CSS
            let mut background_resources = Vec::new();
            page_html_parser.extract_background_images_from_css(&css_content, &mut background_resources);
            
            // Download ALL background images to ensure CSS renders properly
            for resource in &background_resources {
                // Always download background images from any site
                // This ensures the CSS renders without 404 errors
                if let Err(e) = Self::download_resource(
                    client,
                    file_manager,
                    &page_html_parser,
                    &resource.original_url,
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
    ) -> Result<()> {
        // Skip if already downloaded
        if file_manager.file_exists(url) {
            return Ok(());
        }
        
        let response = match client.get(url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                eprintln!("❌ Failed to send request for resource {}: {}", url, e);
                return Ok(());
            }
        };
        
        if response.status() != StatusCode::OK {
            eprintln!("⚠️  HTTP {} for resource {}", response.status(), url);
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
                eprintln!("❌ Failed to read resource body {}: {}", url, e);
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
        
        let _saved_path = match file_manager.save_file(&local_path, &content, Some(&content_type)) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("❌ Failed to save resource {}: {}", url, e);
                return Ok(());
            }
        };
        
        println!("✅ Downloaded: {}", url);
        
        Ok(())
    }
} 
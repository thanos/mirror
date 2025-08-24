use anyhow::Result;
use reqwest::{Client, ClientBuilder, StatusCode};
use std::collections::{HashSet, HashMap, BinaryHeap};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use std::path::{Path, PathBuf};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::cmp::Ordering;

use std::fs;
use serde::{Serialize, Deserialize};

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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PersistentStore {
    pub downloads: HashMap<String, String>, // URL -> local path mapping
    pub last_updated: std::time::SystemTime,
}

impl PersistentStore {
    pub fn new() -> Self {
        Self {
            downloads: HashMap::new(),
            last_updated: std::time::SystemTime::now(),
        }
    }
    
    pub fn load(store_path: &Path) -> Result<Self> {
        if store_path.exists() {
            let content = fs::read_to_string(store_path)?;
            let store: PersistentStore = serde_json::from_str(&content)?;
            Ok(store)
        } else {
            Ok(Self::new())
        }
    }
    
    pub fn save(&self, store_path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(store_path, content)?;
        Ok(())
    }
    
    pub fn clear(&mut self) {
        self.downloads.clear();
        self.last_updated = std::time::SystemTime::now();
    }
    
    pub fn add_download(&mut self, url: String, local_path: String) {
        self.downloads.insert(url, local_path);
        self.last_updated = std::time::SystemTime::now();
    }
    
    pub fn has_download(&self, url: &str) -> bool {
        self.downloads.contains_key(url)
    }
    
    pub fn get_local_path(&self, url: &str) -> Option<&String> {
        self.downloads.get(url)
    }
}

#[derive(Clone, Debug)]
pub struct WebsiteMirror {
    pub base_url: String,
    pub output_dir: PathBuf,
    pub max_depth: usize,
    pub max_concurrent: usize,
    pub ignore_robots: bool,
    pub download_external: bool,
    pub only_resources: Option<Vec<String>>,
    pub convert_to_webp: bool,
    client: Client,
    file_manager: FileManager,
    html_parser: HtmlParser,
    visited_urls: Arc<Mutex<HashSet<String>>>,
    download_queue: Arc<Mutex<BinaryHeap<DownloadTask>>>,
    semaphore: Arc<Semaphore>,
    persistent_store: Arc<Mutex<PersistentStore>>,
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
    pub fn get_local_path_for_resource_static(html_parser: &HtmlParser, original_url: &str, convert_to_webp: bool, current_html_path: &str) -> Result<String> {
        let local_path = html_parser.url_to_local_path_string(original_url)?;
        
        // Convert image extensions to WebP for JPEG/PNG files if the flag is enabled
        // Skip conversion if the image is already WebP
        let final_local_path = if convert_to_webp && 
            (original_url.ends_with(".jpg") || original_url.ends_with(".jpeg") || original_url.ends_with(".png") ||
             original_url.ends_with(".JPG") || original_url.ends_with(".JPEG") || original_url.ends_with(".PNG")) &&
            !(original_url.ends_with(".webp") || original_url.ends_with(".WEBP")) {
            // Convert extension to .webp (only for JPEG/PNG, not already WebP)
            local_path.replace(".jpg", ".webp")
                     .replace(".jpeg", ".webp")
                     .replace(".png", ".webp")
                     .replace(".JPG", ".webp")
                     .replace(".JPEG", ".webp")
                     .replace(".PNG", ".webp")
        } else {
            // Keep original path (including existing WebP images)
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

    /// Perform comprehensive WebP extension replacement for any remaining image references
    /// Only converts JPEG/PNG to WebP, leaves existing WebP files unchanged
    pub fn perform_comprehensive_webp_replacement(html_content: &str) -> String {
        let mut updated_content = html_content.to_string();
        
        // First, do simple string replacements for JPEG/PNG extensions only
        // This catches most cases including those in JavaScript, CSS, and HTML
        // Use a two-pass approach to avoid double-converting already .webp files
        
        // Pass 1: Mark already-converted .webp files with a temporary marker
        updated_content = updated_content.replace(".webp", "___WEBP_MARKER___");
        updated_content = updated_content.replace(".WEBP", "___WEBP_MARKER_UPPER___");
        
        // Pass 2: Convert JPEG/PNG extensions to .webp (but not existing WebP)
        let simple_replacements = vec![
            (".jpg", ".webp"),
            (".jpeg", ".webp"),
            (".png", ".webp"),
            (".JPG", ".webp"),
            (".JPEG", ".webp"),
            (".PNG", ".webp"),
        ];
        
        for (old_ext, new_ext) in simple_replacements {
            let before_count = updated_content.matches(old_ext).count();
            updated_content = updated_content.replace(old_ext, new_ext);
            let after_count = updated_content.matches(new_ext).count();
            
            if before_count > 0 {
                println!("🔍 Simple WebP replacement: {} -> {} ({} replacements)", 
                         old_ext, new_ext, after_count);
            }
        }
        
        // Pass 3: Restore the original .webp files (keep them unchanged)
        updated_content = updated_content.replace("___WEBP_MARKER___", ".webp");
        updated_content = updated_content.replace("___WEBP_MARKER_UPPER___", ".WEBP");
        
        // Then use regex patterns for more specific cases that might have been missed
        // Only target JPEG/PNG, not existing WebP files
        let patterns = vec![
            // URLs in quotes that might have been missed (only JPEG/PNG)
            (r#"url\(["']?([^"']*\.(?:jpg|jpeg|png|JPG|JPEG|PNG))["']?\)"#, r#"url($1.webp)"#),
            // Src attributes that might have been missed (only JPEG/PNG)
            (r#"src=["']([^"']*\.(?:jpg|jpeg|png|JPG|JPEG|PNG))["']"#, r#"src="$1.webp""#),
            // Background image URLs that might have been missed (only JPEG/PNG)
            (r#"background-image:\s*url\(["']?([^"']*\.(?:jpg|jpeg|png|JPG|JPEG|PNG))["']?\)"#, r#"background-image: url($1.webp)"#),
        ];
        
        for (pattern, replacement) in patterns {
            let regex = regex::Regex::new(pattern).unwrap();
            let before_count = regex.find_iter(&updated_content).count();
            updated_content = regex.replace_all(&updated_content, replacement).to_string();
            let after_count = regex.find_iter(&updated_content).count();
            
            if before_count > 0 {
                println!("🔍 Regex WebP replacement: {} -> {} ({} replacements)", 
                         pattern, replacement, after_count);
            }
        }
        
        updated_content
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
        
        // Check if image has transparency (alpha channel)
        let has_transparency = img.color().has_alpha();
        
        let webp_data = if has_transparency {
            // Convert to RGBA8 to preserve transparency
            let rgba_img = img.to_rgba8();
            
            // Create WebP encoder with RGBA (preserves transparency)
            let encoder = Encoder::from_rgba(&rgba_img, rgba_img.width(), rgba_img.height());
            
            // Encode with quality 80 (good balance between size and quality)
            encoder.encode(80.0)
        } else {
            // Convert to RGB8 for images without transparency
            let rgb_img = img.to_rgb8();
            
            // Create WebP encoder with RGB
            let encoder = Encoder::from_rgb(&rgb_img, rgb_img.width(), rgb_img.height());
            
            // Encode with quality 80
            encoder.encode(80.0)
        };
        
        let original_size = image_data.len();
        let webp_size = webp_data.len();
        let compression_ratio = (original_size as f64 / webp_size as f64 * 100.0) as u32;
        
        let transparency_info = if has_transparency { "with transparency" } else { "without transparency" };
        println!("🔄 Converted {} to WebP {}: {} -> {} bytes ({}% of original size)", 
                 original_url, transparency_info, original_size, webp_size, compression_ratio);
        
        Ok(webp_data.to_vec())
    }

    /// Check if a URL belongs to the same domain as the target site
    fn is_same_domain(&self, url: &str) -> bool {
        // Parse the URL to extract the domain
        if let Ok(parsed_url) = url::Url::parse(url) {
            if let Some(host) = parsed_url.host_str() {
                // Check if the host matches the base URL domain
                if let Ok(base_url) = url::Url::parse(&self.base_url) {
                    if let Some(base_host) = base_url.host_str() {
                        // Check for exact match or subdomain
                        return host == base_host || host.ends_with(&format!(".{}", base_host));
                    }
                }
            }
        }
        false
    }

    /// Check if a resource type should be processed based on the only_resources filter
    pub fn should_process_resource_type(&self, resource_type: &ResourceType) -> bool {
        if let Some(ref only_resources) = self.only_resources {
            let type_str = match resource_type {
                ResourceType::Image => "images",
                ResourceType::CSS => "css",
                ResourceType::JavaScript => "js",
                ResourceType::Link => "html",
                ResourceType::PDF => "pdf",
                ResourceType::Video => "video",
                ResourceType::Other => "other",
            };
            only_resources.iter().any(|r| r.to_lowercase() == type_str)
        } else {
            // If no filter specified, process all resource types
            true
        }
    }

    /// Check if a resource should be downloaded based on domain and resource type
    fn should_download_resource(&self, url: &str, resource_type: &ResourceType) -> bool {
        match resource_type {
            // Always download CSS, JS, images, PDFs, and videos from any domain (they're needed for page rendering)
            ResourceType::CSS | ResourceType::JavaScript | ResourceType::Image | ResourceType::PDF | ResourceType::Video => true,
            
            // Only download HTML pages from the target domain
            ResourceType::Link => self.is_same_domain(url),
            
            // Download other resources from any domain
            ResourceType::Other => true,
        }
    }

    /// Rewrite external resources in HTML content that might have been missed during initial parsing
    fn rewrite_external_resources_in_html(&self, html_content: &str, html_parser: &HtmlParser) -> Result<String> {
        let mut updated_html = html_content.to_string();
        
        // Find all image, PDF, and video URLs in the HTML (including those in links, inline styles, etc.)
        let resource_url_patterns = vec![
            // Links to images (like poster downloads)
            (r#"href=["']([^"']*\.(?:jpg|jpeg|png|gif|webp|JPG|JPEG|PNG|GIF|WEBP))["']"#, "href"),
            // Src attributes for images
            (r#"src=["']([^"']*\.(?:jpg|jpeg|png|gif|webp|JPG|JPEG|PNG|GIF|WEBP))["']"#, "src"),
            // Background images in inline styles
            (r#"background-image:\s*url\(["']?([^"']*\.(?:jpg|jpeg|png|gif|webp|JPG|JPEG|PNG|GIF|WEBP))["']?\)"#, "background-image"),
            // Data attributes that might contain image URLs
            (r#"data-[^=]*=["']([^"']*\.(?:jpg|jpeg|png|gif|webp|JPG|JPEG|PNG|GIF|WEBP))["']"#, "data-attribute"),
            // Links to PDFs
            (r#"href=["']([^"']*\.(?:pdf|PDF))["']"#, "href"),
            // Links to videos
            (r#"href=["']([^"']*\.(?:mp4|avi|mov|wmv|flv|webm|mkv|m4v|MP4|AVI|MOV|WMV|FLV|WEBM|MKV|M4V))["']"#, "href"),
            // Video src attributes
            (r#"src=["']([^"']*\.(?:mp4|avi|mov|wmv|flv|webm|mkv|m4v|MP4|AVI|MOV|WMV|FLV|WEBM|MKV|M4V))["']"#, "src"),
        ];
        
        for (pattern, attribute_type) in resource_url_patterns {
            let regex = regex::Regex::new(pattern).unwrap();
            
            // Collect all replacements to avoid borrow checker issues
            let mut replacements = Vec::new();
            
            for captures in regex.captures_iter(&updated_html) {
                if let Some(image_url) = captures.get(1) {
                    let image_url_str = image_url.as_str();
                    
                    // Skip data URLs and relative URLs
                    if image_url_str.starts_with("data:") || image_url_str.starts_with("#") {
                        continue;
                    }
                    
                    // Try to get the local path for this image
                    if let Ok(local_path) = WebsiteMirror::get_local_path_for_resource_static(
                        html_parser, 
                        image_url_str, 
                        self.convert_to_webp, 
                        ""
                    ) {
                                                    // Replace the external URL with the local path
                            let old_url = image_url.as_str();
                            let new_url = if self.convert_to_webp && self.is_resource_url(old_url) && 
                                (old_url.ends_with(".jpg") || old_url.ends_with(".jpeg") || old_url.ends_with(".png") ||
                                 old_url.ends_with(".JPG") || old_url.ends_with(".JPEG") || old_url.ends_with(".PNG")) {
                                // If converting to WebP, ensure the local path has .webp extension (only for JPEG/PNG)
                                self.ensure_webp_extension(&local_path)
                            } else {
                                local_path
                            };
                        
                        replacements.push((old_url.to_string(), new_url));
                    }
                }
            }
            
            // Apply all replacements
            for (old_url, new_url) in replacements {
                println!("🔗 Rewriting {}: {} -> {}", attribute_type, old_url, new_url);
                updated_html = updated_html.replace(&old_url, &new_url);
            }
        }
        
        Ok(updated_html)
    }

    /// Check if a URL is an image, PDF, or video URL
    fn is_resource_url(&self, url: &str) -> bool {
        let resource_extensions = [
            ".jpg", ".jpeg", ".png", ".gif", ".webp", ".JPG", ".JPEG", ".PNG", ".GIF", ".WEBP",
            ".pdf", ".PDF",
            ".mp4", ".avi", ".mov", ".wmv", ".flv", ".webm", ".mkv", ".m4v",
            ".MP4", ".AVI", ".MOV", ".WMV", ".FLV", ".WEBM", ".MKV", ".M4V"
        ];
        resource_extensions.iter().any(|ext| url.ends_with(ext))
    }

    /// Ensure a local path has .webp extension if converting to WebP
    fn ensure_webp_extension(&self, local_path: &str) -> String {
        if !self.convert_to_webp {
            return local_path.to_string();
        }
        
        let image_extensions = [".jpg", ".jpeg", ".png", ".gif", ".JPG", ".JPEG", ".PNG", ".GIF"];
        let mut result = local_path.to_string();
        
        for ext in &image_extensions {
            if result.ends_with(ext) {
                result = result.replace(ext, ".webp");
                break;
            }
        }
        
        result
    }

    /// Extract additional image, PDF, and video URLs from HTML content that might be in links or inline content
    fn extract_additional_image_urls(&self, html_content: &str) -> Vec<String> {
        let mut additional_urls = Vec::new();
        
        // Patterns to find image, PDF, and video URLs in various HTML contexts
        let patterns = vec![
            // Links to images (like poster downloads)
            r#"href=["']([^"']*\.(?:jpg|jpeg|png|gif|webp|JPG|JPEG|PNG|GIF|WEBP))["']"#,
            // Src attributes for images
            r#"src=["']([^"']*\.(?:jpg|jpeg|png|gif|webp|JPG|JPEG|PNG|GIF|WEBP))["']"#,
            // Background images in inline styles
            r#"background-image:\s*url\(["']?([^"']*\.(?:jpg|jpeg|png|gif|webp|JPG|JPEG|PNG|GIF|WEBP))["']?\)"#,
            // Data attributes that might contain image URLs
            r#"data-[^=]*=["']([^"']*\.(?:jpg|jpeg|png|gif|webp|JPG|JPEG|PNG|GIF|WEBP))["']"#,
            // Links to PDFs
            r#"href=["']([^"']*\.(?:pdf|PDF))["']"#,
            // Links to videos
            r#"href=["']([^"']*\.(?:mp4|avi|mov|wmv|flv|webm|mkv|m4v|MP4|AVI|MOV|WMV|FLV|WEBM|MKV|M4V))["']"#,
            // Video src attributes
            r#"src=["']([^"']*\.(?:mp4|avi|mov|wmv|flv|webm|mkv|m4v|MP4|AVI|MOV|WMV|FLV|WEBM|MKV|M4V))["']"#,
        ];
        
        for pattern in patterns {
            if let Ok(regex) = regex::Regex::new(pattern) {
                for captures in regex.captures_iter(html_content) {
                    if let Some(url) = captures.get(1) {
                        let url_str = url.as_str();
                        
                        // Skip data URLs, relative URLs, and already processed URLs
                        if url_str.starts_with("data:") || 
                           url_str.starts_with("#") || 
                           url_str.starts_with("/") {
                            continue;
                        }
                        
                        // Only add if it's an external URL (not from target domain)
                        if !self.is_same_domain(url_str) {
                            additional_urls.push(url_str.to_string());
                        }
                    }
                }
            }
        }
        
        // Remove duplicates
        additional_urls.sort();
        additional_urls.dedup();
        
        additional_urls
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
        clear_store: bool,
    ) -> Result<Self> {
        let client = Self::build_http_client()?;
        let file_manager = FileManager::new(output_dir)?;
        let html_parser = HtmlParser::new(base_url)?;
        
        // Initialize persistent store
        let store_path = output_dir.join(".download_store.json");
        let persistent_store = if clear_store {
            println!("🗑️  Clearing persistent download store");
            PersistentStore::new()
        } else {
            match PersistentStore::load(&store_path) {
                Ok(store) => {
                    println!("📦 Loaded persistent download store with {} entries", store.downloads.len());
                    store
                }
                Err(_) => {
                    println!("📦 Creating new persistent download store");
                    PersistentStore::new()
                }
            }
        };
        
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
            persistent_store: Arc::new(Mutex::new(persistent_store)),
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
        
        // Extract and display target domain
        if let Ok(base_url) = url::Url::parse(&self.base_url) {
            if let Some(host) = base_url.host_str() {
                println!("🎯 Target domain: {} (will mirror HTML pages from this domain only)", host.blue());
                println!("📦 External resources (CSS, JS, images) will be downloaded from any domain");
            }
        }
        
        // Process the base URL first
        if self.only_resources.is_none() || self.should_process_resource_type(&ResourceType::Link) {
            println!("🌐 Processing URL: {}", self.base_url);
            
            if let Err(e) = self.process_page(&self.base_url, 0).await {
                eprintln!("❌ Error processing base URL {}: {}", self.base_url, e);
                return Err(e);
            }
            
            println!("✅ PAGE DONE URL: {}", self.base_url);
        } else {
            println!("🔍 Resource filter active: skipping HTML page crawling");
        }
        
        // Save the persistent store
        let store_path = self.output_dir.join(".download_store.json");
        if let Ok(store) = self.persistent_store.lock() {
            if let Err(e) = store.save(&store_path) {
                eprintln!("⚠️  Warning: Failed to save persistent store: {}", e);
            }
        }
        
        let visited_count = self.visited_urls.lock().unwrap().len();
        println!("📊 Total pages processed: {}", visited_count);
        
        Ok(())
    }
    
    /// Process a single page with the new flow: download dependencies, convert images, update HTML
    async fn process_page(&self, url: &str, depth: usize) -> Result<()> {
        // Check if already visited
        {
            let mut visited = self.visited_urls.lock().unwrap();
            if visited.contains(url) {
                println!("⏭️  Already processed: {}", url);
                return Ok(());
            }
            visited.insert(url.to_string());
        }
        
        // Check depth limit
        if self.max_depth > 0 && depth > self.max_depth {
            println!("🔒 Depth limit reached for: {}", url);
            return Ok(());
        }
        
        println!("📥 Downloading page: {}", url);
        
        // Download the HTML page
        let client = self.client.clone();
        let response = client.get(url).send().await?;
        
        if response.status() != StatusCode::OK {
            eprintln!("⚠️  HTTP {} for {}", response.status(), url);
            return Ok(());
        }
        
        let html_content = response.text().await?;
        println!("📄 HTML downloaded: {} bytes", html_content.len());
        
        // Parse HTML and extract resources
        let html_parser = HtmlParser::new(url)?;
        let resources = html_parser.extract_resources(&html_content)?;
        
                    // Also extract additional resource URLs that might be in links or inline content
            let additional_resource_urls = self.extract_additional_image_urls(&html_content);
        
                    // Separate resources by type and filter based on domain rules
            let mut css_resources = Vec::new();
            let mut js_resources = Vec::new();
            let mut image_resources = Vec::new();
            let mut pdf_resources = Vec::new();
            let mut video_resources = Vec::new();
            let mut link_resources = Vec::new();
            let mut skipped_resources = Vec::new();
            
            // Process regular resources
            for resource in &resources {
                if self.should_download_resource(&resource.original_url, &resource.resource_type) {
                    match resource.resource_type {
                        ResourceType::CSS => css_resources.push(resource.clone()),
                        ResourceType::JavaScript => js_resources.push(resource.clone()),
                        ResourceType::Image => image_resources.push(resource.clone()),
                        ResourceType::PDF => pdf_resources.push(resource.clone()),
                        ResourceType::Video => video_resources.push(resource.clone()),
                        ResourceType::Link => link_resources.push(resource.clone()),
                        _ => {}
                    }
                } else {
                    skipped_resources.push(resource.clone());
                }
            }
        
                    // Add additional resource URLs found in HTML content
            for resource_url in &additional_resource_urls {
                // Determine resource type based on file extension
                let resource_type = if resource_url.ends_with(".pdf") || resource_url.ends_with(".PDF") {
                    ResourceType::PDF
                } else if resource_url.ends_with(".mp4") || resource_url.ends_with(".avi") || 
                          resource_url.ends_with(".mov") || resource_url.ends_with(".wmv") ||
                          resource_url.ends_with(".flv") || resource_url.ends_with(".webm") ||
                          resource_url.ends_with(".mkv") || resource_url.ends_with(".m4v") ||
                          resource_url.ends_with(".MP4") || resource_url.ends_with(".AVI") || 
                          resource_url.ends_with(".MOV") || resource_url.ends_with(".WMV") ||
                          resource_url.ends_with(".FLV") || resource_url.ends_with(".WEBM") ||
                          resource_url.ends_with(".MKV") || resource_url.ends_with(".M4V") {
                    ResourceType::Video
                } else {
                    ResourceType::Image
                };
                
                // Create a resource link for this resource
                if let Ok(resource) = html_parser.create_resource_link(resource_url, resource_type.clone()) {
                    match resource_type {
                        ResourceType::PDF => pdf_resources.push(resource),
                        ResourceType::Video => video_resources.push(resource),
                        ResourceType::Image => image_resources.push(resource),
                        _ => {}
                    }
                    println!("🔍 Found additional {} in HTML: {}", 
                             match resource_type {
                                 ResourceType::PDF => "PDF",
                                 ResourceType::Video => "video",
                                 ResourceType::Image => "image",
                                 _ => "resource"
                             }, resource_url);
                }
            }
        
                    println!("🔍 Found {} CSS, {} JS, {} images, {} PDFs, {} videos, {} links", 
                     css_resources.len(), js_resources.len(), image_resources.len(), 
                     pdf_resources.len(), video_resources.len(), link_resources.len());
        
        if !skipped_resources.is_empty() {
            println!("⏭️  Skipping {} resources (not from target domain or filtered out):", skipped_resources.len());
            for resource in &skipped_resources {
                let reason = if resource.resource_type == ResourceType::Link && !self.is_same_domain(&resource.original_url) {
                    "external HTML page"
                } else {
                    "filtered out"
                };
                println!("  ⏭️  {:?}: {} ({})", resource.resource_type, resource.original_url, reason);
            }
        }
        
        // Download CSS files first (critical for page rendering) - from ANY domain
        println!("📥 Downloading CSS files...");
        for resource in &css_resources {
            let domain_info = if self.is_same_domain(&resource.original_url) { "target domain" } else { "external domain" };
            println!("  📄 CSS from {}: {}", domain_info, resource.original_url);
            self.download_resource(&client, &html_parser, &resource.original_url).await?;
        }
        
        // Download JavaScript files - from ANY domain
        println!("📥 Downloading JavaScript files...");
        for resource in &js_resources {
            let domain_info = if self.is_same_domain(&resource.original_url) { "target domain" } else { "external domain" };
            println!("  📜 JS from {}: {}", domain_info, resource.original_url);
            self.download_resource(&client, &html_parser, &resource.original_url).await?;
        }
        
        // Download and convert images to WebP - from ANY domain
        if self.convert_to_webp {
            println!("🔄 Converting images to WebP...");
        }
        
        let progress_bar = ProgressBar::new(image_resources.len() as u64);
        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner} [{bar:40.cyan/blue}] {pos}/{len} images {msg}")
                .unwrap()
        );
        
        for (i, resource) in image_resources.iter().enumerate() {
            progress_bar.set_position(i as u64);
            let domain_info = if self.is_same_domain(&resource.original_url) { "target domain" } else { "external domain" };
            progress_bar.set_message(format!("Converting {} from {}", 
                resource.original_url.split('/').last().unwrap_or("image"), domain_info));
            
            self.download_resource(&client, &html_parser, &resource.original_url).await?;
        }
                    progress_bar.finish_with_message("✅ All images processed");
            
            // Download PDF files - from ANY domain
            if !pdf_resources.is_empty() {
                println!("📥 Downloading PDF files...");
                for resource in &pdf_resources {
                    let domain_info = if self.is_same_domain(&resource.original_url) { "target domain" } else { "external domain" };
                    println!("  📄 PDF from {}: {}", domain_info, resource.original_url);
                    self.download_resource(&self.client, &html_parser, &resource.original_url).await?;
                }
            }
            
            // Download video files - from ANY domain
            if !video_resources.is_empty() {
                println!("📥 Downloading video files...");
                for resource in &video_resources {
                    let domain_info = if self.is_same_domain(&resource.original_url) { "target domain" } else { "external domain" };
                    println!("  🎥 Video from {}: {}", domain_info, resource.original_url);
                    self.download_resource(&self.client, &html_parser, &resource.original_url).await?;
                }
            }
            
            // Update HTML to use local references
            println!("🔧 Updating HTML references...");
        let mut updated_html = html_content.clone();
        
        // Update CSS references
        for resource in &css_resources {
            if let Ok(local_path) = WebsiteMirror::get_local_path_for_resource_static(&html_parser, &resource.original_url, false, "") {
                updated_html = updated_html.replace(&resource.original_url, &local_path);
            }
        }
        
        // Update JS references
        for resource in &js_resources {
            if let Ok(local_path) = WebsiteMirror::get_local_path_for_resource_static(&html_parser, &resource.original_url, false, "") {
                updated_html = updated_html.replace(&resource.original_url, &local_path);
            }
        }
        
                    // Update image references
            for resource in &image_resources {
                if let Ok(local_path) = WebsiteMirror::get_local_path_for_resource_static(&html_parser, &resource.original_url, self.convert_to_webp, "") {
                    updated_html = updated_html.replace(&resource.original_url, &local_path);
                }
            }
            
            // Update PDF references
            for resource in &pdf_resources {
                if let Ok(local_path) = WebsiteMirror::get_local_path_for_resource_static(&html_parser, &resource.original_url, false, "") {
                    updated_html = updated_html.replace(&resource.original_url, &local_path);
                }
            }
            
            // Update video references
            for resource in &video_resources {
                if let Ok(local_path) = WebsiteMirror::get_local_path_for_resource_static(&html_parser, &resource.original_url, false, "") {
                    updated_html = updated_html.replace(&resource.original_url, &local_path);
                }
            }
        
        // Apply comprehensive WebP replacement if enabled
        if self.convert_to_webp {
            updated_html = Self::perform_comprehensive_webp_replacement(&updated_html);
        }
        
        // Additional HTML rewriting for external resources that might have been missed
        // This catches poster links, inline image references, and other external resources
        updated_html = self.rewrite_external_resources_in_html(&updated_html, &html_parser)?;
        
        // Save the updated HTML
        let local_path = html_parser.url_to_local_path_string(url)?;
        self.file_manager.save_file(&local_path, updated_html.as_bytes(), Some("text/html"))?;
        println!("💾 HTML saved: {}", local_path);
        
        // Process linked pages recursively - ONLY from the target domain
        for resource in &link_resources {
            // Only process HTML pages from the target domain
            if self.is_same_domain(&resource.original_url) {
                if let Err(e) = Box::pin(self.process_page(&resource.original_url, depth + 1)).await {
                    eprintln!("⚠️  Failed to process linked page {}: {}", resource.original_url, e);
                }
            } else {
                println!("⏭️  Skipping external HTML page: {} (not from target domain)", resource.original_url);
            }
        }
        
        Ok(())
    }
    
    async fn download_resource(
        &self,
        client: &Client,
        html_parser: &HtmlParser,
        url: &str,
    ) -> Result<()> {
        // Check if already downloaded using persistent store
        {
            let store = self.persistent_store.lock().unwrap();
            if store.has_download(url) {
                let cached_path = store.get_local_path(url).unwrap();
                println!("⏭️  Skipping {} (already downloaded to {})", url, cached_path);
                return Ok(());
            }
        }
        
        // Check if file exists on disk
        if self.file_manager.file_exists(url) {
            // Add to persistent store for future reference
            let local_path = html_parser.url_to_local_path_string(url)?;
            let mut store = self.persistent_store.lock().unwrap();
            store.add_download(url.to_string(), local_path.clone());
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
        // Skip conversion if the image is already WebP
        let (final_content, final_content_type, final_local_path) = if self.convert_to_webp && 
            (url.ends_with(".jpg") || url.ends_with(".jpeg") || url.ends_with(".png") ||
             url.ends_with(".JPG") || url.ends_with(".JPEG") || url.ends_with(".PNG")) &&
            !(url.ends_with(".webp") || url.ends_with(".WEBP")) {
            
            // Convert to WebP (only for JPEG/PNG, not already WebP images)
            let webp_data = Self::convert_to_webp_static(&content, url)?;
            
            // Change file extension to .webp (handle both lowercase and uppercase)
            let webp_path = local_path.replace(".jpg", ".webp")
                                    .replace(".jpeg", ".webp")
                                    .replace(".png", ".webp")
                                    .replace(".JPG", ".webp")
                                    .replace(".JPEG", ".webp")
                                    .replace(".PNG", ".webp");
            
            (webp_data, "image/webp".to_string(), webp_path)
        } else {
            // Keep original content and path (including existing WebP images)
            if url.ends_with(".webp") || url.ends_with(".WEBP") {
                println!("📋 Copying WebP image as-is (no conversion needed): {}", url);
            }
            (content.to_vec(), content_type, local_path.clone())
        };
        
        // For WebP conversion, we need to save the file with the .webp extension
        // but also ensure the path matches what will be used in HTML rewriting
        let save_path = if self.convert_to_webp && 
            (url.ends_with(".jpg") || url.ends_with(".jpeg") || url.ends_with(".png") ||
             url.ends_with(".JPG") || url.ends_with(".JPEG") || url.ends_with(".PNG")) &&
            !(url.ends_with(".webp") || url.ends_with(".WEBP")) {
            // Use the .webp path for saving (only for converted images)
            final_local_path.clone()
        } else {
            // Use the original path for saving (including existing WebP images)
            local_path.clone()
        };
        
        let saved_path = match self.file_manager.save_file(&save_path, &final_content, Some(&final_content_type)) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("❌ Failed to save {} {}: {}", resource_type, url, e);
                return Ok(());
            }
        };
        
        // Add to persistent store - use the save_path to ensure consistency
        {
            let mut store = self.persistent_store.lock().unwrap();
            store.add_download(url.to_string(), save_path.to_string());
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
            false,
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
            true,
            false
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
            false,
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
            false,
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
            false,
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
            false,
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
use anyhow::{Result, Context};
use select::document::Document;
use select::predicate::{Name, Attr};
use url::Url;

#[derive(Debug, Clone)]
pub struct ResourceLink {
    pub original_url: String,
    pub local_path: String,
    pub resource_type: ResourceType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResourceType {
    CSS,
    JavaScript,
    Image,
    Link,
    Other,
}

#[derive(Clone)]
pub struct HtmlParser {
    base_url: Url,
}

impl HtmlParser {
    pub fn new(base_url: &str) -> Result<Self> {
        let base_url = Url::parse(base_url)
            .with_context(|| format!("Failed to parse base URL: {}", base_url))?;
        
        Ok(Self { base_url })
    }
    
    pub fn extract_resources(&self, html_content: &str) -> Result<Vec<ResourceLink>> {
        let document = Document::from(html_content);
        let mut resources = Vec::new();
        
        // Extract CSS files
        for link in document.find(Name("link")) {
            if let Some(href) = link.attr("href") {
                if let Some(rel) = link.attr("rel") {
                    if rel.contains("stylesheet") {
                        if let Ok(resource) = self.create_resource_link(href, ResourceType::CSS) {
                            resources.push(resource);
                        }
                    }
                }
            }
        }
        
        // Extract JavaScript files
        for script in document.find(Name("script")) {
            if let Some(src) = script.attr("src") {
                if let Ok(resource) = self.create_resource_link(src, ResourceType::JavaScript) {
                    resources.push(resource);
                }
            }
        }
        
        // Extract images
        for img in document.find(Name("img")) {
            if let Some(src) = img.attr("src") {
                if let Ok(resource) = self.create_resource_link(src, ResourceType::Image) {
                    resources.push(resource);
                }
            }
        }
        
        // Extract background images from inline styles
        for element in document.find(Attr("style", ())) {
            if let Some(style) = element.attr("style") {
                self.extract_background_images_from_css(style, &mut resources);
            }
        }
        
        // Extract background images from CSS files
        for link in document.find(Name("link")) {
            if let Some(href) = link.attr("href") {
                if let Some(rel) = link.attr("rel") {
                    if rel.contains("stylesheet") {
                        // Mark CSS files for background image extraction
                        if let Ok(resource) = self.create_resource_link(href, ResourceType::CSS) {
                            resources.push(resource);
                        }
                    }
                }
            }
        }
        
        // Extract links
        for link in document.find(Name("a")) {
            if let Some(href) = link.attr("href") {
                if let Ok(resource) = self.create_resource_link(href, ResourceType::Link) {
                    resources.push(resource);
                }
            }
        }
        Ok(resources)
    }
    
    fn create_resource_link(&self, url: &str, resource_type: ResourceType) -> Result<ResourceLink> {
        let absolute_url = self.resolve_url(url)?;
        let local_path = self.url_to_local_path(&absolute_url)?;
        
        Ok(ResourceLink {
            original_url: absolute_url.to_string(),
            local_path,
            resource_type,
        })
    }
    
    fn resolve_url(&self, url: &str) -> Result<Url> {
        if url.starts_with("http://") || url.starts_with("https://") {
            Ok(Url::parse(url)?)
        } else if url.starts_with("//") {
            // Protocol-relative URL
            let scheme = self.base_url.scheme();
            let url_with_scheme = format!("{}:{}", scheme, url);
            Ok(Url::parse(&url_with_scheme)?)
        } else {
            // Relative URL
            Ok(self.base_url.join(url)?)
        }
    }
    
    fn url_to_local_path(&self, url: &Url) -> Result<String> {
        let mut path = url.path().to_string();
        
        // Remove leading slash
        if path.starts_with('/') {
            path = path[1..].to_string();
        }
        
        // Handle root path
        if path.is_empty() {
            path = "index.html".to_string();
        } else if path.ends_with('/') {
            path.push_str("index.html");
        } else if !path.contains('.') {
            // No file extension, assume it's a directory
            path.push_str("/index.html");
        }
        
        // Add query parameters if they exist
        if let Some(query) = url.query() {
            if !query.is_empty() {
                path = format!("{}?{}", path, query);
            }
        }
        
        // Sanitize the path for filesystem
        path = self.sanitize_path(&path);
        
        Ok(path)
    }
    
    fn sanitize_path(&self, path: &str) -> String {
        path.chars()
            .map(|c| match c {
                '?' | '&' | '=' | '#' => '_',
                ' ' => '_',
                c if c.is_ascii_alphanumeric() || c == '/' || c == '.' || c == '-' => c,
                _ => '_',
            })
            .collect()
    }
    
    pub fn convert_html_links(&self, html_content: &str) -> Result<String> {
        let document = Document::from(html_content);
        let mut modified_html = html_content.to_string();
        
        // Convert CSS links
        for link in document.find(Name("link")) {
            if let Some(href) = link.attr("href") {
                if let Some(rel) = link.attr("rel") {
                    if rel.contains("stylesheet") {
                        if let Ok(local_path) = self.convert_url_to_local(href) {
                            modified_html = modified_html.replace(
                                &format!("href=\"{}\"", href),
                                &format!("href=\"{}\"", local_path)
                            );
                        }
                    }
                }
            }
        }
        
        // Convert JavaScript links
        for script in document.find(Name("script")) {
            if let Some(src) = script.attr("src") {
                if let Ok(local_path) = self.convert_url_to_local(src) {
                    modified_html = modified_html.replace(
                        &format!("src=\"{}\"", src),
                        &format!("src=\"{}\"", local_path)
                    );
                }
            }
        }
        
        // Convert image links
        for img in document.find(Name("img")) {
            if let Some(src) = img.attr("src") {
                if let Ok(local_path) = self.convert_url_to_local(src) {
                    modified_html = modified_html.replace(
                        &format!("src=\"{}\"", src),
                        &format!("src=\"{}\"", local_path)
                    );
                }
            }
        }
        
        // Convert anchor links
        for link in document.find(Name("a")) {
            if let Some(href) = link.attr("href") {
                if let Ok(local_path) = self.convert_url_to_local(href) {
                    modified_html = modified_html.replace(
                        &format!("href=\"{}\"", href),
                        &format!("href=\"{}\"", local_path)
                    );
                }
            }
        }
        
        Ok(modified_html)
    }
    
    fn convert_url_to_local(&self, url: &str) -> Result<String> {
        let absolute_url = self.resolve_url(url)?;
        let local_path = self.url_to_local_path(&absolute_url)?;
        
        // Convert to relative path
        if local_path.starts_with("index.html") {
            Ok("./".to_string())
        } else {
            Ok(format!("./{}", local_path))
        }
    }
    
    pub fn url_to_local_path_string(&self, url: &str) -> Result<String> {
        if url.starts_with("http://") || url.starts_with("https://") {
            let parsed_url = Url::parse(url)?;
            self.url_to_local_path(&parsed_url)
        } else {
            // For relative URLs, resolve them first
            let absolute_url = self.resolve_url(url)?;
            self.url_to_local_path(&absolute_url)
        }
    }
    
    pub fn extract_background_images_from_css(&self, css_content: &str, resources: &mut Vec<ResourceLink>) {
        // Extract background-image URLs from CSS content
        let background_patterns = [
            r#"background-image:\s*url\(['"]?([^'")\s]+)['"]?\)"#,
            r#"background:\s*url\(['"]?([^'")\s]+)['"]?\)"#,
            r#"background-image:\s*url\(['"]?([^'")\s]+)['"]?\)"#,
        ];
        
        for pattern in &background_patterns {
            if let Ok(regex) = regex::Regex::new(pattern) {
                for cap in regex.captures_iter(css_content) {
                    if let Some(url) = cap.get(1) {
                        if let Ok(resource) = self.create_resource_link(url.as_str(), ResourceType::Image) {
                            resources.push(resource);
                        }
                    }
                }
            }
        }
    }
} 
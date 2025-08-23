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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceType {
    CSS,
    JavaScript,
    Image,
    Link,
    Other,
}

#[derive(Clone)]
#[derive(Debug)]
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
    
    pub fn resolve_url(&self, url: &str) -> Result<Url> {
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
    
    pub fn sanitize_path(&self, path: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_new_html_parser() {
        let parser = HtmlParser::new("https://example.com/page").unwrap();
        assert_eq!(parser.base_url.as_str(), "https://example.com/page");
    }

    #[test]
    fn test_new_html_parser_invalid_url() {
        let result = HtmlParser::new("not-a-url");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_resources() {
        let html_content = r#"
            <html>
                <head>
                    <link rel="stylesheet" href="/static/style.css">
                    <script src="/static/script.js"></script>
                </head>
                <body>
                    <img src="/static/image.jpg" alt="test">
                    <a href="/page">Link</a>
                </body>
            </html>
        "#;
        
        let parser = HtmlParser::new("https://example.com").unwrap();
        let resources = parser.extract_resources(html_content).unwrap();
        
        assert_eq!(resources.len(), 4);
        
        let css_resource = resources.iter().find(|r| r.resource_type == ResourceType::CSS).unwrap();
        assert_eq!(css_resource.original_url, "/static/style.css");
        
        let js_resource = resources.iter().find(|r| r.resource_type == ResourceType::JavaScript).unwrap();
        assert_eq!(js_resource.original_url, "/static/script.js");
        
        let img_resource = resources.iter().find(|r| r.resource_type == ResourceType::Image).unwrap();
        assert_eq!(img_resource.original_url, "/static/image.jpg");
        
        let link_resource = resources.iter().find(|r| r.resource_type == ResourceType::Link).unwrap();
        assert_eq!(link_resource.original_url, "/page");
    }

    #[test]
    fn test_extract_resources_with_absolute_urls() {
        let html_content = r#"
            <html>
                <head>
                    <link rel="stylesheet" href="https://cdn.example.com/style.css">
                    <script src="https://cdn.example.com/script.js"></script>
                </head>
                <body>
                    <img src="https://cdn.example.com/image.jpg" alt="test">
                </body>
            </html>
        "#;
        
        let parser = HtmlParser::new("https://example.com").unwrap();
        let resources = parser.extract_resources(html_content).unwrap();
        
        assert_eq!(resources.len(), 3);
        
        let css_resource = resources.iter().find(|r| r.resource_type == ResourceType::CSS).unwrap();
        assert_eq!(css_resource.original_url, "https://cdn.example.com/style.css");
    }

    #[test]
    fn test_extract_resources_with_relative_urls() {
        let html_content = r#"
            <html>
                <head>
                    <link rel="stylesheet" href="../style.css">
                    <script src="./script.js"></script>
                </head>
                <body>
                    <img src="images/photo.jpg" alt="test">
                </body>
            </html>
        "#;
        
        let parser = HtmlParser::new("https://example.com/subdir/").unwrap();
        let resources = parser.extract_resources(html_content).unwrap();
        
        assert_eq!(resources.len(), 3);
    }

    #[test]
    fn test_extract_resources_with_data_urls() {
        let html_content = r#"
            <html>
                <body>
                    <img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==" alt="test">
                </body>
            </html>
        "#;
        
        let parser = HtmlParser::new("https://example.com").unwrap();
        let resources = parser.extract_resources(html_content).unwrap();
        
        // Data URLs should be ignored
        assert_eq!(resources.len(), 0);
    }

    #[test]
    fn test_extract_resources_with_malformed_html() {
        let html_content = r#"
            <html>
                <head>
                    <link rel="stylesheet" href="/static/style.css
                    <script src="/static/script.js
                </head>
                <body>
                    <img src="/static/image.jpg alt="test">
                </body>
            </html>
        "#;
        
        let parser = HtmlParser::new("https://example.com").unwrap();
        let resources = parser.extract_resources(html_content).unwrap();
        
        // Should still extract what it can
        assert!(resources.len() > 0);
    }

    #[test]
    fn test_url_to_local_path_string_absolute() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.url_to_local_path_string("https://example.com/image.jpg").unwrap();
        assert_eq!(result, "image.jpg");
    }

    #[test]
    fn test_url_to_local_path_string_relative() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.url_to_local_path_string("/image.jpg").unwrap();
        assert_eq!(result, "image.jpg");
    }

    #[test]
    fn test_url_to_local_path_string_root() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.url_to_local_path_string("https://example.com/").unwrap();
        assert_eq!(result, "index.html");
    }

    #[test]
    fn test_url_to_local_path_string_directory() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.url_to_local_path_string("https://example.com/dir/").unwrap();
        assert_eq!(result, "dir/index.html");
    }

    #[test]
    fn test_url_to_local_path_string_no_extension() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.url_to_local_path_string("https://example.com/page").unwrap();
        assert_eq!(result, "page/index.html");
    }

    #[test]
    fn test_url_to_local_path_string_with_query() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.url_to_local_path_string("https://example.com/page?param=value").unwrap();
        assert_eq!(result, "page/index.html?param=value");
    }

    #[test]
    fn test_sanitize_path() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        
        assert_eq!(parser.sanitize_path("normal/path"), "normal/path");
        assert_eq!(parser.sanitize_path("path with spaces"), "path_with_spaces");
        assert_eq!(parser.sanitize_path("path?with=query"), "path_with_query");
        assert_eq!(parser.sanitize_path("path#fragment"), "path_fragment");
        assert_eq!(parser.sanitize_path("path&with&ampersands"), "path_with_ampersands");
    }

    #[test]
    fn test_resolve_url_absolute() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.resolve_url("https://cdn.example.com/style.css").unwrap();
        assert_eq!(result.as_str(), "https://cdn.example.com/style.css");
    }

    #[test]
    fn test_resolve_url_relative() {
        let parser = HtmlParser::new("https://example.com/subdir/").unwrap();
        let result = parser.resolve_url("../style.css").unwrap();
        assert_eq!(result.as_str(), "https://example.com/style.css");
    }

    #[test]
    fn test_resolve_url_protocol_relative() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.resolve_url("//cdn.example.com/style.css").unwrap();
        assert_eq!(result.as_str(), "https://cdn.example.com/style.css");
    }

    #[test]
    fn test_resolve_url_invalid() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.resolve_url("not-a-url");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_background_images_from_css() {
        let css_content = r#"
            .bg1 { background-image: url('/images/bg1.jpg'); }
            .bg2 { background: url('/images/bg2.jpg'); }
            .bg3 { background-image: url('/images/bg3.jpg'); }
        "#;
        
        let parser = HtmlParser::new("https://example.com").unwrap();
        let mut resources = Vec::new();
        parser.extract_background_images_from_css(css_content, &mut resources);
        
        assert_eq!(resources.len(), 3);
        
        let urls: Vec<String> = resources.iter().map(|r| r.original_url.clone()).collect();
        assert!(urls.contains(&"/images/bg1.jpg".to_string()));
        assert!(urls.contains(&"/images/bg2.jpg".to_string()));
        assert!(urls.contains(&"/images/bg3.jpg".to_string()));
    }

    #[test]
    fn test_extract_background_images_from_css_with_quotes() {
        let css_content = r#"
            .bg1 { background-image: url("/images/bg1.jpg"); }
            .bg2 { background: url('/images/bg2.jpg'); }
        "#;
        
        let parser = HtmlParser::new("https://example.com").unwrap();
        let mut resources = Vec::new();
        parser.extract_background_images_from_css(css_content, &mut resources);
        
        assert_eq!(resources.len(), 2);
    }

    #[test]
    fn test_extract_background_images_from_css_no_matches() {
        let css_content = r#"
            .bg1 { background-color: red; }
            .bg2 { color: blue; }
        "#;
        
        let parser = HtmlParser::new("https://example.com").unwrap();
        let mut resources = Vec::new();
        parser.extract_background_images_from_css(css_content, &mut resources);
        
        assert_eq!(resources.len(), 0);
    }

    #[test]
    fn test_create_resource_link() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        
        let resource = parser.create_resource_link("/style.css", ResourceType::CSS).unwrap();
        assert_eq!(resource.original_url, "/style.css");
        assert_eq!(resource.resource_type, ResourceType::CSS);
        
        let resource = parser.create_resource_link("/script.js", ResourceType::JavaScript).unwrap();
        assert_eq!(resource.original_url, "/script.js");
        assert_eq!(resource.resource_type, ResourceType::JavaScript);
        
        let resource = parser.create_resource_link("/image.jpg", ResourceType::Image).unwrap();
        assert_eq!(resource.original_url, "/image.jpg");
        assert_eq!(resource.resource_type, ResourceType::Image);
        
        let resource = parser.create_resource_link("/page", ResourceType::Link).unwrap();
        assert_eq!(resource.original_url, "/page");
        assert_eq!(resource.resource_type, ResourceType::Link);
    }

    #[test]
    fn test_create_resource_link_with_data_url() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.create_resource_link("data:image/png;base64,data", ResourceType::Image);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_resource_link_with_fragment() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.create_resource_link("#fragment", ResourceType::Link);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_resource_link_with_mailto() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.create_resource_link("mailto:test@example.com", ResourceType::Link);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_resource_link_with_tel() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.create_resource_link("tel:+1234567890", ResourceType::Link);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_resource_link_with_javascript() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let result = parser.create_resource_link("javascript:alert('test')", ResourceType::Link);
        assert!(result.is_err());
    }

    #[test]
    fn test_resource_link_clone() {
        let resource = ResourceLink {
            original_url: "/test.css".to_string(),
            local_path: "/local/test.css".to_string(),
            resource_type: ResourceType::CSS,
        };
        
        let cloned = resource.clone();
        assert_eq!(cloned.original_url, resource.original_url);
        assert_eq!(cloned.local_path, resource.local_path);
        assert_eq!(cloned.resource_type, resource.resource_type);
    }

    #[test]
    fn test_resource_type_debug() {
        assert_eq!(format!("{:?}", ResourceType::CSS), "CSS");
        assert_eq!(format!("{:?}", ResourceType::JavaScript), "JavaScript");
        assert_eq!(format!("{:?}", ResourceType::Image), "Image");
        assert_eq!(format!("{:?}", ResourceType::Link), "Link");
        assert_eq!(format!("{:?}", ResourceType::Other), "Other");
    }

    #[test]
    fn test_resource_type_clone() {
        let css_type = ResourceType::CSS;
        let cloned = css_type.clone();
        assert_eq!(cloned, css_type);
    }

    #[test]
    fn test_html_parser_debug() {
        let parser = HtmlParser::new("https://example.com").unwrap();
        let debug_str = format!("{:?}", parser);
        assert!(debug_str.contains("HtmlParser"));
        assert!(debug_str.contains("example.com"));
    }
} 
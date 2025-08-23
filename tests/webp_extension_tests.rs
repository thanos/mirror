use website_mirror::{WebsiteMirror, HtmlParser, FileManager, ResourceType};
use tempfile::tempdir;
use std::fs;

/// Regression test for the WebP extension rewriting bug
/// This test verifies that when --convert-to-webp is enabled, image references
/// in HTML are properly updated to use .webp extensions
#[test]
fn test_webp_extension_rewriting_in_html() {
    let temp_dir = tempdir().unwrap();
    
    // Create a test HTML file with various image references
    let test_html = r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>Test Page</title>
        </head>
        <body>
            <h1>Test Images</h1>
            <img src="https://example.com/image1.jpg" alt="Image 1">
            <img src="https://example.com/image2.jpeg" alt="Image 2">
            <img src="https://example.com/image3.png" alt="Image 3">
            <img src="/local/image4.jpg" alt="Image 4">
            <img src="images/photo.jpeg" alt="Photo">
            <img src="../assets/logo.png" alt="Logo">
            <img src="https://cdn.example.com/banner.jpg" alt="Banner">
        </body>
        </html>
    "#;
    
    // Create a WebsiteMirror instance with WebP conversion enabled
    let mirror = WebsiteMirror::new(
        "https://example.com",
        temp_dir.path(),
        3,
        10,
        false,
        false,
        None,
        true // Enable WebP conversion
    ).unwrap();
    
    // Create an HTML parser for the test page
    let html_parser = HtmlParser::new("https://example.com/test").unwrap();
    
    // Extract resources from the HTML
    let resources = html_parser.extract_resources(test_html).unwrap();
    
    // Filter to get only image resources
    let image_resources: Vec<_> = resources.iter()
        .filter(|r| r.resource_type == ResourceType::Image)
        .collect();
    
    assert_eq!(image_resources.len(), 7, "Should find 7 image resources");
    
    // Test that each image resource would be converted to WebP
    for resource in &image_resources {
        let local_path = WebsiteMirror::get_local_path_for_resource_static(
            &html_parser,
            &resource.original_url,
            true, // convert_to_webp = true
            "test/index.html" // current HTML path
        ).unwrap();
        
        // Verify that the local path has .webp extension
        assert!(
            local_path.ends_with(".webp"),
            "Image {} should have .webp extension, got: {}",
            resource.original_url,
            local_path
        );
        
        // Verify that the original extension was replaced
        assert!(
            !local_path.contains(".jpg") && !local_path.contains(".jpeg") && !local_path.contains(".png"),
            "Image {} should not contain original extension in local path: {}",
            resource.original_url,
            local_path
        );
    }
}

/// Test that HTML content is properly rewritten with WebP extensions
#[test]
fn test_html_content_webp_rewriting() {
    let temp_dir = tempdir().unwrap();
    
    let test_html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <img src="https://example.com/photo.jpg" alt="Photo">
            <img src="https://example.com/logo.png" alt="Logo">
            <img src="https://example.com/banner.jpeg" alt="Banner">
        </body>
        </html>
    "#;
    
    let html_parser = HtmlParser::new("https://example.com").unwrap();
    let resources = html_parser.extract_resources(test_html).unwrap();
    
    // Simulate the HTML rewriting process
    let mut html_content_updated = test_html.to_string();
    
    for resource in &resources {
        if resource.resource_type == ResourceType::Image {
            // Get the local path with WebP conversion
            let local_path = WebsiteMirror::get_local_path_for_resource_static(
                &html_parser,
                &resource.original_url,
                true, // convert_to_webp = true
                "index.html"
            ).unwrap();
            
            // Replace the original URL with the local path
            html_content_updated = html_content_updated.replace(&resource.original_url, &local_path);
            
            // Also handle extension replacement for WebP conversion
            if resource.original_url.ends_with(".jpg") || resource.original_url.ends_with(".jpeg") || resource.original_url.ends_with(".png") {
                let old_extension = if resource.original_url.ends_with(".jpg") {
                    ".jpg"
                } else if resource.original_url.ends_with(".jpeg") {
                    ".jpeg"
                } else {
                    ".png"
                };
                
                // Extract filename for extension replacement
                if let Some(filename) = resource.original_url.split('/').last() {
                    let new_filename = filename.replace(old_extension, ".webp");
                    let old_filename_with_path = resource.original_url;
                    let new_filename_with_path = resource.original_url.replace(filename, &new_filename);
                    
                    // Replace the filename with .webp extension
                    html_content_updated = html_content_updated.replace(&old_filename_with_path, &new_filename_with_path);
                }
            }
        }
    }
    
    // Verify that all image references now use .webp extensions
    assert!(
        html_content_updated.contains("photo.webp"),
        "HTML should contain photo.webp reference"
    );
    assert!(
        html_content_updated.contains("logo.webp"),
        "HTML should contain logo.webp reference"
    );
    assert!(
        html_content_updated.contains("banner.webp"),
        "HTML should contain banner.webp reference"
    );
    
    // Verify that original extensions are no longer present
    assert!(
        !html_content_updated.contains(".jpg"),
        "HTML should not contain .jpg extensions"
    );
    assert!(
        !html_content_updated.contains(".jpeg"),
        "HTML should not contain .jpeg extensions"
    );
    assert!(
        !html_content_updated.contains(".png"),
        "HTML should not contain .png extensions"
    );
    
    println!("Updated HTML content:");
    println!("{}", html_content_updated);
}

/// Test that non-image resources are not affected by WebP conversion
#[test]
fn test_non_image_resources_unaffected_by_webp() {
    let temp_dir = tempdir().unwrap();
    
    let test_html = r#"
        <!DOCTYPE html>
        <html>
        <head>
            <link rel="stylesheet" href="https://example.com/style.css">
            <script src="https://example.com/script.js"></script>
        </head>
        <body>
            <img src="https://example.com/photo.jpg" alt="Photo">
            <a href="https://example.com/page.html">Link</a>
        </body>
        </html>
    "#;
    
    let html_parser = HtmlParser::new("https://example.com").unwrap();
    let resources = html_parser.extract_resources(test_html).unwrap();
    
    for resource in &resources {
        let local_path = WebsiteMirror::get_local_path_for_resource_static(
            &html_parser,
            &resource.original_url,
            true, // convert_to_webp = true
            "index.html"
        ).unwrap();
        
        match resource.resource_type {
            ResourceType::Image => {
                // Images should be converted to .webp
                if resource.original_url.ends_with(".jpg") || resource.original_url.ends_with(".jpeg") || resource.original_url.ends_with(".png") {
                    assert!(
                        local_path.ends_with(".webp"),
                        "Image {} should have .webp extension",
                        resource.original_url
                    );
                }
            },
            ResourceType::CSS => {
                // CSS files should keep their original extension
                assert!(
                    local_path.ends_with(".css"),
                    "CSS file {} should keep .css extension",
                    resource.original_url
                );
            },
            ResourceType::JavaScript => {
                // JS files should keep their original extension
                assert!(
                    local_path.ends_with(".js"),
                    "JS file {} should keep .js extension",
                    resource.original_url
                );
            },
            ResourceType::Link => {
                // HTML links should keep their original extension
                assert!(
                    local_path.ends_with(".html"),
                    "HTML link {} should keep .html extension",
                    resource.original_url
                );
            },
            _ => {}
        }
    }
}

/// Test edge cases for WebP extension conversion
#[test]
fn test_webp_extension_edge_cases() {
    let temp_dir = tempdir().unwrap();
    let html_parser = HtmlParser::new("https://example.com").unwrap();
    
    let test_cases = vec![
        ("image.jpg", "image.webp"),
        ("photo.jpeg", "photo.webp"),
        ("logo.png", "logo.webp"),
        ("file.JPG", "file.webp"), // Uppercase extension
        ("file.JPEG", "file.webp"), // Uppercase extension
        ("file.PNG", "file.webp"), // Uppercase extension
        ("path/to/image.jpg", "path/to/image.webp"), // With path
        ("https://cdn.com/image.jpg", "image.webp"), // Full URL
        ("/local/image.jpg", "local/image.webp"), // Absolute path
        ("../assets/image.jpg", "../assets/image.webp"), // Relative path
    ];
    
    for (input_url, expected_filename) in test_cases {
        let local_path = WebsiteMirror::get_local_path_for_resource_static(
            &html_parser,
            input_url,
            true, // convert_to_webp = true
            "index.html"
        ).unwrap();
        
        // The local path should end with the expected filename
        assert!(
            local_path.ends_with(expected_filename),
            "URL {} should result in local path ending with {}, got: {}",
            input_url,
            expected_filename,
            local_path
        );
    }
}

/// Test that WebP conversion is disabled when the flag is false
#[test]
fn test_webp_conversion_disabled() {
    let temp_dir = tempdir().unwrap();
    let html_parser = HtmlParser::new("https://example.com").unwrap();
    
    let test_urls = vec![
        "https://example.com/image.jpg",
        "https://example.com/photo.jpeg",
        "https://example.com/logo.png",
    ];
    
    for url in test_urls {
        let local_path = WebsiteMirror::get_local_path_for_resource_static(
            &html_parser,
            url,
            false, // convert_to_webp = false
            "index.html"
        ).unwrap();
        
        // When WebP conversion is disabled, extensions should remain unchanged
        if url.ends_with(".jpg") {
            assert!(
                local_path.ends_with(".jpg"),
                "URL {} should keep .jpg extension when WebP conversion is disabled",
                url
            );
        } else if url.ends_with(".jpeg") {
            assert!(
                local_path.ends_with(".jpeg"),
                "URL {} should keep .jpeg extension when WebP conversion is disabled",
                url
            );
        } else if url.ends_with(".png") {
            assert!(
                local_path.ends_with(".png"),
                "URL {} should keep .png extension when WebP conversion is disabled",
                url
            );
        }
    }
}

/// Test the complete HTML rewriting workflow with WebP conversion
#[test]
fn test_complete_html_rewriting_workflow() {
    let temp_dir = tempdir().unwrap();
    
    // Simulate a real HTML page with mixed content
    let original_html = r#"
        <!DOCTYPE html>
        <html>
        <head>
            <link rel="stylesheet" href="https://cdn.example.com/style.css">
            <script src="https://cdn.example.com/script.js"></script>
        </head>
        <body>
            <header>
                <img src="https://example.com/logo.jpg" alt="Logo">
                <img src="https://example.com/banner.png" alt="Banner">
            </header>
            <main>
                <img src="https://example.com/hero.jpeg" alt="Hero Image">
                <img src="https://example.com/thumbnail.jpg" alt="Thumbnail">
            </main>
            <footer>
                <img src="https://example.com/footer-logo.png" alt="Footer Logo">
            </footer>
        </body>
        </html>
    "#;
    
    let html_parser = HtmlParser::new("https://example.com").unwrap();
    let resources = html_parser.extract_resources(original_html).unwrap();
    
    let mut html_content_updated = original_html.to_string();
    
    // Process each resource type
    for resource in &resources {
        match resource.resource_type {
            ResourceType::Image => {
                // Get local path with WebP conversion
                let local_path = WebsiteMirror::get_local_path_for_resource_static(
                    &html_parser,
                    &resource.original_url,
                    true, // convert_to_webp = true
                    "index.html"
                ).unwrap();
                
                // Replace the original URL with the local path
                html_content_updated = html_content_updated.replace(&resource.original_url, &local_path);
                
                // Handle WebP extension replacement
                if resource.original_url.ends_with(".jpg") || resource.original_url.ends_with(".jpeg") || resource.original_url.ends_with(".png") {
                    let old_extension = if resource.original_url.ends_with(".jpg") {
                        ".jpg"
                    } else if resource.original_url.ends_with(".jpeg") {
                        ".jpeg"
                    } else {
                        ".png"
                    };
                    
                    if let Some(filename) = resource.original_url.split('/').last() {
                        let new_filename = filename.replace(old_extension, ".webp");
                        let old_filename_with_path = resource.original_url;
                        let new_filename_with_path = resource.original_url.replace(filename, &new_filename);
                        
                        html_content_updated = html_content_updated.replace(&old_filename_with_path, &new_filename_with_path);
                    }
                }
            },
            ResourceType::CSS => {
                // CSS files should be converted to local paths but keep .css extension
                let local_path = html_parser.url_to_local_path_string(&resource.original_url).unwrap();
                html_content_updated = html_content_updated.replace(&resource.original_url, &local_path);
            },
            ResourceType::JavaScript => {
                // JS files should be converted to local paths but keep .js extension
                let local_path = html_parser.url_to_local_path_string(&resource.original_url).unwrap();
                html_content_updated = html_content_updated.replace(&resource.original_url, &local_path);
            },
            _ => {}
        }
    }
    
    // Verify the final result
    println!("Final HTML content:");
    println!("{}", html_content_updated);
    
    // Check that all image references use .webp extensions
    assert!(html_content_updated.contains("logo.webp"));
    assert!(html_content_updated.contains("banner.webp"));
    assert!(html_content_updated.contains("hero.webp"));
    assert!(html_content_updated.contains("thumbnail.webp"));
    assert!(html_content_updated.contains("footer-logo.webp"));
    
    // Check that CSS and JS files keep their extensions
    assert!(html_content_updated.contains("style.css"));
    assert!(html_content_updated.contains("script.js"));
    
    // Check that original extensions are no longer present
    assert!(!html_content_updated.contains(".jpg"));
    assert!(!html_content_updated.contains(".jpeg"));
    assert!(!html_content_updated.contains(".png"));
} 
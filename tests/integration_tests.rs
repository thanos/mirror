use std::path::Path;
use website_mirror::{WebsiteMirror, HtmlParser, FileManager, ResourceType, DownloadTask, DownloadPriority};
use tempfile::tempdir;
use std::fs;

#[test]
fn test_basic_mirror_setup() {
    let temp_dir = tempdir().unwrap();
    let mirror = WebsiteMirror::new(
        "https://example.com",
        temp_dir.path().to_str().unwrap(),
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
}

#[test]
fn test_html_parser_integration() {
    let html_content = r#"
        <html>
            <head>
                <link rel="stylesheet" href="/style.css">
                <script src="/script.js"></script>
            </head>
            <body>
                <img src="/image.jpg" alt="test">
                <a href="/page">Link</a>
            </body>
        </html>
    "#;
    
    let parser = HtmlParser::new("https://example.com").unwrap();
    let resources = parser.extract_resources(html_content).unwrap();
    
    assert_eq!(resources.len(), 4);
    
    let css_count = resources.iter().filter(|r| r.resource_type == ResourceType::CSS).count();
    let js_count = resources.iter().filter(|r| r.resource_type == ResourceType::JavaScript).count();
    let img_count = resources.iter().filter(|r| r.resource_type == ResourceType::Image).count();
    let link_count = resources.iter().filter(|r| r.resource_type == ResourceType::Link).count();
    
    assert_eq!(css_count, 1);
    assert_eq!(js_count, 1);
    assert_eq!(img_count, 1);
    assert_eq!(link_count, 1);
}

#[test]
fn test_file_manager_integration() {
    let temp_dir = tempdir().unwrap();
    let file_manager = FileManager::new(temp_dir.path()).unwrap();
    
    // Test saving multiple files
    let files = vec![
        ("test1.txt", b"Content 1", Some("text/plain")),
        ("subdir/test2.txt", b"Content 2", Some("text/plain")),
        ("test3.html", b"<html>Content 3</html>", Some("text/html")),
    ];
    
    for (path, content, content_type) in files {
        let result = file_manager.save_file(path, content, content_type);
        assert!(result.is_ok(), "Failed to save {}", path);
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists(), "File {} was not created", path);
        
        let read_content = fs::read(&saved_path).unwrap();
        assert_eq!(read_content, content, "Content mismatch for {}", path);
    }
    
    // Check directory structure
    let subdir = temp_dir.path().join("subdir");
    assert!(subdir.exists() && subdir.is_dir());
}

#[test]
fn test_resource_type_filtering() {
    let temp_dir = tempdir().unwrap();
    
    // Test with no restrictions
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
        Some(vec!["images".to_string()]),
        false
    ).unwrap();
    
    assert!(!mirror.should_process_resource_type(&ResourceType::CSS));
    assert!(!mirror.should_process_resource_type(&ResourceType::JavaScript));
    assert!(mirror.should_process_resource_type(&ResourceType::Image));
    assert!(!mirror.should_process_resource_type(&ResourceType::Link));
    
    // Test with multiple restrictions
    let mirror = WebsiteMirror::new(
        "https://example.com",
        temp_dir.path(),
        3,
        10,
        false,
        false,
        Some(vec!["css".to_string(), "js".to_string()]),
        false
    ).unwrap();
    
    assert!(mirror.should_process_resource_type(&ResourceType::CSS));
    assert!(mirror.should_process_resource_type(&ResourceType::JavaScript));
    assert!(!mirror.should_process_resource_type(&ResourceType::Image));
    assert!(!mirror.should_process_resource_type(&ResourceType::Link));
}

#[test]
fn test_webp_conversion_flag() {
    let temp_dir = tempdir().unwrap();
    
    let mirror = WebsiteMirror::new(
        "https://example.com",
        temp_dir.path(),
        3,
        10,
        false,
        false,
        None,
        true
    ).unwrap();
    
    assert!(mirror.convert_to_webp);
}

#[test]
fn test_full_mirror_options() {
    let temp_dir = tempdir().unwrap();
    
    let mirror = WebsiteMirror::new(
        "https://example.com",
        temp_dir.path(),
        100, // max_depth
        50,  // max_concurrent
        true, // ignore_robots
        true, // download_external
        None,
        false
    ).unwrap();
    
    assert_eq!(mirror.max_depth, 100);
    assert_eq!(mirror.max_concurrent, 50);
    assert!(mirror.ignore_robots);
    assert!(mirror.download_external);
}

#[test]
fn test_path_sanitization() {
    let parser = HtmlParser::new("https://example.com").unwrap();
    
    let test_cases = vec![
        ("normal/path", "normal/path"),
        ("path with spaces", "path_with_spaces"),
        ("path?with=query", "path_with_query"),
        ("path#fragment", "path_fragment"),
        ("path&with&ampersands", "path_with_ampersands"),
        ("path=with=equals", "path_with_equals"),
    ];
    
    for (input, expected) in test_cases {
        let result = parser.sanitize_path(input);
        assert_eq!(result, expected, "Failed for input: {}", input);
    }
}

#[test]
fn test_url_resolution() {
    let parser = HtmlParser::new("https://example.com/subdir/").unwrap();
    
    let test_cases = vec![
        ("../style.css", "https://example.com/style.css"),
        ("./script.js", "https://example.com/subdir/script.js"),
        ("images/photo.jpg", "https://example.com/subdir/images/photo.jpg"),
        ("https://cdn.example.com/style.css", "https://cdn.example.com/style.css"),
        ("//cdn.example.com/script.js", "https://cdn.example.com/script.js"),
    ];
    
    for (input, expected) in test_cases {
        let result = parser.resolve_url(input).unwrap();
        assert_eq!(result.as_str(), expected, "Failed for input: {}", input);
    }
}

#[test]
fn test_css_background_image_extraction() {
    let css_content = r#"
        .bg1 { background-image: url('/images/bg1.jpg'); }
        .bg2 { background: url('/images/bg2.jpg'); }
        .bg3 { background-image: url('/images/bg3.jpg'); }
        .bg4 { background-color: red; }
        .bg5 { color: blue; }
    "#;
    
    let parser = HtmlParser::new("https://example.com").unwrap();
    let mut resources = Vec::new();
    parser.extract_background_images_from_css(css_content, &mut resources);
    
    assert_eq!(resources.len(), 3);
    
    let urls: Vec<String> = resources.iter().map(|r| r.original_url.clone()).collect();
    assert!(urls.contains(&"/images/bg1.jpg".to_string()));
    assert!(urls.contains(&"/images/bg2.jpg".to_string()));
    assert!(urls.contains(&"/images/bg3.jpg".to_string()));
    assert!(!urls.contains(&"/images/bg4.jpg".to_string()));
}

#[test]
fn test_download_task_priority_queue() {
    use std::collections::BinaryHeap;
    use website_mirror::DownloadTask;
    use website_mirror::DownloadPriority;
    
    let mut queue = BinaryHeap::new();
    
    // Add tasks in random order
    queue.push(DownloadTask {
        url: "https://example.com/image.jpg".to_string(),
        depth: 2,
        priority: DownloadPriority::Normal,
        resource_type: Some(ResourceType::Image),
    });
    
    queue.push(DownloadTask {
        url: "https://example.com/style.css".to_string(),
        depth: 1,
        priority: DownloadPriority::Critical,
        resource_type: Some(ResourceType::CSS),
    });
    
    queue.push(DownloadTask {
        url: "https://example.com/page.html".to_string(),
        depth: 1,
        priority: DownloadPriority::High,
        resource_type: Some(ResourceType::Link),
    });
    
    queue.push(DownloadTask {
        url: "https://example.com/script.js".to_string(),
        depth: 2,
        priority: DownloadPriority::Critical,
        resource_type: Some(ResourceType::JavaScript),
    });
    
    // Tasks should come out in priority order
    let first = queue.pop().unwrap();
    assert_eq!(first.priority, DownloadPriority::Critical);
    assert_eq!(first.depth, 1); // Lower depth should come first for same priority
    
    let second = queue.pop().unwrap();
    assert_eq!(second.priority, DownloadPriority::Critical);
    assert_eq!(second.depth, 2);
    
    let third = queue.pop().unwrap();
    assert_eq!(third.priority, DownloadPriority::High);
    
    let fourth = queue.pop().unwrap();
    assert_eq!(fourth.priority, DownloadPriority::Normal);
}

/// Test that WebP extension rewriting works correctly in the actual download process
#[test]
fn test_webp_extension_rewriting_integration() {
    let temp_dir = tempdir().unwrap();
    
    // Create a test HTML file with image references
    let test_html = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <img src="https://example.com/test-image.jpg" alt="Test Image">
            <img src="https://example.com/another-image.png" alt="Another Image">
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
    
    // Create an HTML parser
    let html_parser = HtmlParser::new("https://example.com").unwrap();
    
    // Extract resources
    let resources = html_parser.extract_resources(test_html).unwrap();
    
    // Filter to get only image resources
    let image_resources: Vec<_> = resources.iter()
        .filter(|r| r.resource_type == ResourceType::Image)
        .collect();
    
    assert_eq!(image_resources.len(), 2, "Should find 2 image resources");
    
    // Test that the mirror correctly identifies which resources should be processed
    for resource in &image_resources {
        assert!(
            mirror.should_process_resource_type(&resource.resource_type),
            "Image resource {} should be processed",
            resource.original_url
        );
    }
    
    // Test that local paths are correctly generated with WebP extensions
    for resource in &image_resources {
        let local_path = WebsiteMirror::get_local_path_for_resource_static(
            &html_parser,
            &resource.original_url,
            true, // convert_to_webp = true
            "index.html"
        ).unwrap();
        
        // Verify WebP extension conversion
        if resource.original_url.ends_with(".jpg") {
            assert!(
                local_path.ends_with(".webp"),
                "JPG image {} should be converted to .webp, got: {}",
                resource.original_url,
                local_path
                );
        } else if resource.original_url.ends_with(".png") {
            assert!(
                local_path.ends_with(".webp"),
                "PNG image {} should be converted to .webp, got: {}",
                resource.original_url,
                local_path
                );
        }
    }
} 
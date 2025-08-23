use criterion::{black_box, criterion_group, criterion_main, Criterion};
use website_mirror::{HtmlParser, FileManager, ResourceType};
use tempfile::tempdir;

fn bench_html_parsing(c: &mut Criterion) {
    let html_content = r#"
        <html>
            <head>
                <link rel="stylesheet" href="/style.css">
                <script src="/script.js"></script>
                <link rel="stylesheet" href="/theme.css">
                <script src="/utils.js"></script>
            </head>
            <body>
                <img src="/logo.png" alt="Logo">
                <img src="/banner.jpg" alt="Banner">
                <a href="/about">About</a>
                <a href="/contact">Contact</a>
                <a href="/products">Products</a>
            </body>
        </html>
    "#;
    
    let parser = HtmlParser::new("https://example.com").unwrap();
    
    c.bench_function("parse_html_resources", |b| {
        b.iter(|| {
            let _resources = parser.extract_resources(black_box(html_content)).unwrap();
        });
    });
}

fn bench_path_sanitization(c: &mut Criterion) {
    let parser = HtmlParser::new("https://example.com").unwrap();
    let test_paths = vec![
        "normal/path",
        "path with spaces",
        "path?with=query",
        "path#fragment",
        "path&with&ampersands",
        "path=with=equals",
        "path/with/multiple/special/chars?param=value&other=123#fragment",
    ];
    
    c.bench_function("sanitize_paths", |b| {
        b.iter(|| {
            for path in &test_paths {
                let _sanitized = parser.sanitize_path(black_box(path));
            }
        });
    });
}

fn bench_url_resolution(c: &mut Criterion) {
    let parser = HtmlParser::new("https://example.com/subdir/").unwrap();
    let test_urls = vec![
        "../style.css",
        "./script.js",
        "images/photo.jpg",
        "https://cdn.example.com/style.css",
        "//cdn.example.com/script.js",
        "../../../assets/logo.png",
        "./nested/path/file.css",
    ];
    
    c.bench_function("resolve_urls", |b| {
        b.iter(|| {
            for url in &test_urls {
                let _resolved = parser.resolve_url(black_box(url)).unwrap();
            }
        });
    });
}

fn bench_css_background_extraction(c: &mut Criterion) {
    let css_content = r#"
        .bg1 { background-image: url('/images/bg1.jpg'); }
        .bg2 { background: url('/images/bg2.jpg'); }
        .bg3 { background-image: url('/images/bg3.jpg'); }
        .bg4 { background: url('/images/bg4.jpg'); }
        .bg5 { background: url('/images/bg5.jpg'); }
        .bg6 { background-color: red; }
        .bg7 { color: blue; }
        .bg8 { background: url('/images/bg8.jpg'); }
        .bg9 { background-image: url('/images/bg9.jpg'); }
        .bg10 { background: url('/images/bg10.jpg'); }
    "#;
    
    let parser = HtmlParser::new("https://example.com").unwrap();
    
    c.bench_function("extract_css_backgrounds", |b| {
        b.iter(|| {
            let mut resources = Vec::new();
            parser.extract_background_images_from_css(black_box(css_content), &mut resources);
        });
    });
}

fn bench_file_saving(c: &mut Criterion) {
    let temp_dir = tempdir().unwrap();
    let file_manager = FileManager::new(temp_dir.path()).unwrap();
    let test_content = b"This is test content for benchmarking file saving operations";
    
    c.bench_function("save_single_file", |b| {
        b.iter(|| {
            let _result = file_manager.save_file("benchmark.txt", black_box(test_content), Some("text/plain"));
        });
    });
}

fn bench_multiple_file_saving(c: &mut Criterion) {
    let temp_dir = tempdir().unwrap();
    let file_manager = FileManager::new(temp_dir.path()).unwrap();
    let test_files = vec![
        ("file1.txt", b"Content 1"),
        ("subdir/file2.txt", b"Content 2"),
        ("nested/path/file3.txt", b"Content 3"),
        ("file4.html", b"<html>Content 4</html>"),
        ("assets/style.css", b"body { color: red; }"),
    ];
    
    c.bench_function("save_multiple_files", |b| {
        b.iter(|| {
            for (path, content) in &test_files {
                let _result = file_manager.save_file(black_box(path), black_box(content), Some("text/plain"));
            }
        });
    });
}

fn bench_resource_type_filtering(c: &mut Criterion) {
    let temp_dir = tempdir().unwrap();
    let mirror = website_mirror::WebsiteMirror::new(
        "https://example.com",
        temp_dir.path().to_str().unwrap(),
        3,
        10,
        false,
        false,
        Some(vec!["images".to_string(), "css".to_string()]),
        false
    ).unwrap();
    
    let resource_types = vec![
        ResourceType::CSS,
        ResourceType::JavaScript,
        ResourceType::Image,
        ResourceType::Link,
        ResourceType::Other,
    ];
    
    c.bench_function("filter_resource_types", |b| {
        b.iter(|| {
            for resource_type in &resource_types {
                let _should_process = mirror.should_process_resource_type(black_box(resource_type));
            }
        });
    });
}

criterion_group!(
    benches,
    bench_html_parsing,
    bench_path_sanitization,
    bench_url_resolution,
    bench_css_background_extraction,
    bench_file_saving,
    bench_multiple_file_saving,
    bench_resource_type_filtering,
);
criterion_main!(benches); 
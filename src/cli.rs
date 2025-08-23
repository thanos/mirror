use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "website-mirror",
    about = "A CLI utility to mirror websites by downloading static copies",
    version,
    long_about = "Downloads a static, page-by-page copy of a website's HTML, CSS, images, and JavaScript files. Converts links to work locally and creates necessary directories."
)]
pub struct MirrorCommand {
    /// The URL of the website to mirror
    #[arg(required = true)]
    pub url: String,
    
    /// Output directory for the mirrored website
    #[arg(short, long, default_value = "./mirrored_site")]
    pub output_dir: PathBuf,
    
    /// Maximum depth for crawling (0 = unlimited)
    #[arg(short = 'd', long, default_value = "3")]
    pub max_depth: usize,
    
    /// Maximum concurrent downloads
    #[arg(short = 'c', long, default_value = "10")]
    pub max_concurrent: usize,
    
    /// Ignore robots.txt and download all pages
    #[arg(short = 'r', long)]
    pub ignore_robots: bool,
    
    /// User agent string to use for requests
    #[arg(long, default_value = "WebsiteMirror/1.0")]
    pub user_agent: String,
    
                /// Download external resources (CSS, JS, images from other domains)
            /// Note: All media files (images, CSS, JS) are always downloaded to ensure pages render properly
            #[arg(short = 'e', long)]
            pub download_external: bool,
    
    /// Follow redirects
    #[arg(long, default_value = "true")]
    pub follow_redirects: bool,
    
    /// Timeout for requests in seconds
    #[arg(long, default_value = "30")]
    pub timeout: u64,
    
                /// Full recursive mirror with all options enabled
            #[arg(long)]
            pub full_mirror: bool,

            /// Mirror only specific resource types (comma-separated: images,css,js,html)
            /// Examples: --only-resources images,css or --only-resources js
            #[arg(long, value_delimiter = ',')]
            pub only_resources: Option<Vec<String>>,

            /// Convert JPEG/PNG images to WebP format for better compression
            #[arg(long)]
            pub convert_to_webp: bool,
} 
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_args() {
        let args = MirrorCommand::try_parse_from(&[
            "website-mirror",
            "https://example.com",
            "-o", "./output"
        ]).unwrap();
        
        assert_eq!(args.url, "https://example.com");
        assert_eq!(args.output_dir, "./output");
        assert_eq!(args.max_depth, 3);
        assert_eq!(args.max_concurrent, 10);
        assert_eq!(args.ignore_robots, false);
        assert_eq!(args.download_external, false);
        assert_eq!(args.convert_to_webp, false);
    }

    #[test]
    fn test_parse_all_args() {
        let args = MirrorCommand::try_parse_from(&[
            "website-mirror",
            "https://example.com",
            "-o", "./output",
            "-d", "5",
            "-c", "20",
            "--ignore-robots",
            "--download-external",
            "--convert-to-webp"
        ]).unwrap();
        
        assert_eq!(args.url, "https://example.com");
        assert_eq!(args.output_dir, "./output");
        assert_eq!(args.max_depth, 5);
        assert_eq!(args.max_concurrent, 20);
        assert_eq!(args.ignore_robots, true);
        assert_eq!(args.download_external, true);
        assert_eq!(args.convert_to_webp, true);
    }

    #[test]
    fn test_parse_only_resources() {
        let args = MirrorCommand::try_parse_from(&[
            "website-mirror",
            "https://example.com",
            "-o", "./output",
            "--only-resources", "images,css"
        ]).unwrap();
        
        assert_eq!(args.only_resources, Some(vec!["images".to_string(), "css".to_string()]));
    }

    #[test]
    fn test_parse_single_resource() {
        let args = MirrorCommand::try_parse_from(&[
            "website-mirror",
            "https://example.com",
            "-o", "./output",
            "--only-resources", "js"
        ]).unwrap();
        
        assert_eq!(args.only_resources, Some(vec!["js".to_string()]));
    }

    #[test]
    fn test_parse_full_mirror() {
        let args = MirrorCommand::try_parse_from(&[
            "website-mirror",
            "https://example.com",
            "-o", "./output",
            "--full-mirror"
        ]).unwrap();
        
        assert_eq!(args.max_depth, 100);
        assert_eq!(args.max_concurrent, 50);
        assert_eq!(args.ignore_robots, true);
        assert_eq!(args.download_external, true);
    }

    #[test]
    fn test_parse_missing_url() {
        let result = MirrorCommand::try_parse_from(&[
            "website-mirror",
            "-o", "./output"
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_output() {
        let result = MirrorCommand::try_parse_from(&[
            "website-mirror",
            "https://example.com"
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_depth() {
        let result = MirrorCommand::try_parse_from(&[
            "website-mirror",
            "https://example.com",
            "-o", "./output",
            "-d", "0"
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_concurrent() {
        let result = MirrorCommand::try_parse_from(&[
            "website-mirror",
            "https://example.com",
            "-o", "./output",
            "-c", "0"
        ]);
        assert!(result.is_err());
    }
} 
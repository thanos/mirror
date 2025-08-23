use clap::Parser;
use anyhow::Result;

mod downloader;
mod html_parser;
mod file_manager;
mod cli;

use cli::MirrorCommand;
use downloader::WebsiteMirror;

#[tokio::main]
async fn main() -> Result<()> {
    let args = MirrorCommand::parse();
    
    // Handle full mirror option
    let (max_depth, max_concurrent, ignore_robots, download_external) = if args.full_mirror {
        // Full mirror: unlimited depth crawling of target site + all media files from any site
        (0, 100, true, true)
    } else {
        // Standard mirror: limited depth + all media files from any site (ensures no 404s)
        (args.max_depth, args.max_concurrent, args.ignore_robots, true)
    };
    
                let mut mirror = WebsiteMirror::new(
                &args.url,
                &args.output_dir,
                max_depth,
                max_concurrent,
                ignore_robots,
                download_external,
                args.only_resources.clone(),
                args.convert_to_webp,
            )?;
    
    mirror.mirror_website().await?;
    
    println!("✅ Website mirroring completed successfully!");
    Ok(())
} 

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_args() {
        let args = vec![
            "website-mirror".to_string(),
            "https://example.com".to_string(),
            "-o".to_string(),
            "./output".to_string(),
        ];
        
        let result = MirrorCommand::try_parse_from(args);
        assert!(result.is_ok());
        
        let cmd = result.unwrap();
        assert_eq!(cmd.url, "https://example.com");
        assert_eq!(cmd.output_dir, "./output");
    }

    #[test]
    fn test_parse_args_with_full_mirror() {
        let args = vec![
            "website-mirror".to_string(),
            "https://example.com".to_string(),
            "-o".to_string(),
            "./output".to_string(),
            "--full-mirror".to_string(),
        ];
        
        let result = MirrorCommand::try_parse_from(args);
        assert!(result.is_ok());
        
        let cmd = result.unwrap();
        assert_eq!(cmd.url, "https://example.com");
        assert_eq!(cmd.output_dir, "./output");
        assert!(cmd.full_mirror);
    }

    #[test]
    fn test_parse_args_with_convert_to_webp() {
        let args = vec![
            "website-mirror".to_string(),
            "https://example.com".to_string(),
            "-o".to_string(),
            "./output".to_string(),
            "--convert-to-webp".to_string(),
        ];
        
        let result = MirrorCommand::try_parse_from(args);
        assert!(result.is_ok());
        
        let cmd = result.unwrap();
        assert_eq!(cmd.url, "https://example.com");
        assert_eq!(cmd.output_dir, "./output");
        assert!(cmd.convert_to_webp);
    }

    #[test]
    fn test_parse_args_with_only_resources() {
        let args = vec![
            "website-mirror".to_string(),
            "https://example.com".to_string(),
            "-o".to_string(),
            "./output".to_string(),
            "--only-resources".to_string(),
            "images,css".to_string(),
        ];
        
        let result = MirrorCommand::try_parse_from(args);
        assert!(result.is_ok());
        
        let cmd = result.unwrap();
        assert_eq!(cmd.url, "https://example.com");
        assert_eq!(cmd.output_dir, "./output");
        assert_eq!(cmd.only_resources, Some(vec!["images".to_string(), "css".to_string()]));
    }

    #[test]
    fn test_parse_args_with_depth_and_concurrent() {
        let args = vec![
            "website-mirror".to_string(),
            "https://example.com".to_string(),
            "-o".to_string(),
            "./output".to_string(),
            "-d".to_string(),
            "5".to_string(),
            "-c".to_string(),
            "20".to_string(),
        ];
        
        let result = MirrorCommand::try_parse_from(args);
        assert!(result.is_ok());
        
        let cmd = result.unwrap();
        assert_eq!(cmd.url, "https://example.com");
        assert_eq!(cmd.output_dir, "./output");
        assert_eq!(cmd.max_depth, 5);
        assert_eq!(cmd.max_concurrent, 20);
    }

    #[test]
    fn test_parse_args_with_ignore_robots() {
        let args = vec![
            "website-mirror".to_string(),
            "https://example.com".to_string(),
            "-o".to_string(),
            "./output".to_string(),
            "--ignore-robots".to_string(),
        ];
        
        let result = MirrorCommand::try_parse_from(args);
        assert!(result.is_ok());
        
        let cmd = result.unwrap();
        assert_eq!(cmd.url, "https://example.com");
        assert_eq!(cmd.output_dir, "./output");
        assert!(cmd.ignore_robots);
    }

    #[test]
    fn test_parse_args_with_download_external() {
        let args = vec![
            "website-mirror".to_string(),
            "https://example.com".to_string(),
            "-o".to_string(),
            "./output".to_string(),
            "--download-external".to_string(),
        ];
        
        let result = MirrorCommand::try_parse_from(args);
        assert!(result.is_ok());
        
        let cmd = result.unwrap();
        assert_eq!(cmd.url, "https://example.com");
        assert_eq!(cmd.output_dir, "./output");
        assert!(cmd.download_external);
    }

    #[test]
    fn test_parse_args_missing_url() {
        let args = vec![
            "website-mirror".to_string(),
            "-o".to_string(),
            "./output".to_string(),
        ];
        
        let result = MirrorCommand::try_parse_from(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_args_missing_output() {
        let args = vec![
            "website-mirror".to_string(),
            "https://example.com".to_string(),
        ];
        
        let result = MirrorCommand::try_parse_from(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_args_invalid_depth() {
        let args = vec![
            "website-mirror".to_string(),
            "https://example.com".to_string(),
            "-o".to_string(),
            "./output".to_string(),
            "-d".to_string(),
            "0".to_string(),
        ];
        
        let result = MirrorCommand::try_parse_from(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_args_invalid_concurrent() {
        let args = vec![
            "website-mirror".to_string(),
            "https://example.com".to_string(),
            "-o".to_string(),
            "./output".to_string(),
            "-c".to_string(),
            "0".to_string(),
        ];
        
        let result = MirrorCommand::try_parse_from(args);
        assert!(result.is_err());
    }
} 
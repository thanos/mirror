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
    )?;
    
    mirror.mirror_website().await?;
    
    println!("✅ Website mirroring completed successfully!");
    Ok(())
} 
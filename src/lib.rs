pub mod cli;
pub mod downloader;
pub mod file_manager;
pub mod html_parser;

// Re-export main types for convenience
pub use cli::MirrorCommand;
pub use downloader::{WebsiteMirror, DownloadTask, DownloadPriority};
pub use file_manager::FileManager;
pub use html_parser::{HtmlParser, ResourceType, ResourceLink}; 
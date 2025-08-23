use anyhow::{Result, Context};
use std::path::{Path, PathBuf};
use std::fs;
use mime_guess::MimeGuess;
use std::io::Write;

#[derive(Clone)]
pub struct FileManager {
    base_dir: PathBuf,
}

impl FileManager {
    pub fn new(base_dir: &Path) -> Result<Self> {
        let base_dir = base_dir.to_path_buf();
        fs::create_dir_all(&base_dir)
            .with_context(|| format!("Failed to create base directory: {:?}", base_dir))?;
        
        Ok(Self { base_dir })
    }
    
    pub fn create_directories_for_url(&self, url_path: &str) -> Result<PathBuf> {
        let mut path = self.base_dir.clone();
        
        // Split the URL path and create directories
        for segment in url_path.split('/').filter(|s| !s.is_empty()) {
            path.push(segment);
        }
        
        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {:?}", parent))?;
        }
        
        Ok(path)
    }
    
    pub fn save_file(&self, url_path: &str, content: &[u8], mime_type: Option<&str>) -> Result<PathBuf> {
        let mut file_path = self.create_directories_for_url(url_path)?;
        
        // Determine file extension based on MIME type or content
        let extension = self.get_file_extension(url_path, mime_type, content);
        if !extension.is_empty() {
            file_path.set_extension(extension);
        }
        
        // Write the file
        let mut file = fs::File::create(&file_path)
            .with_context(|| format!("Failed to create file: {:?}", file_path))?;
        
        file.write_all(content)
            .with_context(|| format!("Failed to write to file: {:?}", file_path))?;
        
        Ok(file_path)
    }
    
    fn get_file_extension(&self, url_path: &str, mime_type: Option<&str>, content: &[u8]) -> String {
        // First try to get extension from MIME type
        if let Some(mime) = mime_type {
            if let Some(ext) = MimeGuess::from_path(mime).first() {
                return ext.to_string();
            }
        }
        
        // Check if content looks like HTML
        if content.starts_with(b"<!DOCTYPE") || content.starts_with(b"<html") {
            return "html".to_string();
        }
        
        // Try to get extension from URL path
        if let Some(ext) = Path::new(url_path).extension() {
            return ext.to_string_lossy().to_string();
        }
        
        // Default to no extension
        String::new()
    }
    
    pub fn get_relative_path(&self, file_path: &Path) -> Result<PathBuf> {
        file_path.strip_prefix(&self.base_dir)
            .map(|p| p.to_path_buf())
            .with_context(|| format!("Failed to get relative path from {:?}", file_path))
    }
    
    pub fn file_exists(&self, url_path: &str) -> bool {
        let mut path = self.base_dir.clone();
        for segment in url_path.split('/').filter(|s| !s.is_empty()) {
            path.push(segment);
        }
        path.exists()
    }
} 
use anyhow::{Result, Context};
use std::path::{Path, PathBuf};
use std::fs;
use mime_guess::MimeGuess;
use std::io::Write;

#[derive(Clone)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn test_new_file_manager() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        assert_eq!(file_manager.base_dir, temp_dir.path());
    }

    #[test]
    fn test_new_file_manager_invalid_path() {
        let result = FileManager::new(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn test_save_file() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"Hello, World!";
        let result = file_manager.save_file("test.txt", content, Some("text/plain"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        let read_content = fs::read(&saved_path).unwrap();
        assert_eq!(read_content, content);
    }

    #[test]
    fn test_save_file_with_subdirectories() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"CSS content";
        let result = file_manager.save_file("css/style.css", content, Some("text/css"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Check that subdirectory was created
        let css_dir = temp_dir.path().join("css");
        assert!(css_dir.exists());
        assert!(css_dir.is_dir());
    }

    #[test]
    fn test_save_file_without_content_type() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"Binary data";
        let result = file_manager.save_file("data.bin", content, None);
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
    }

    #[test]
    fn test_save_file_with_special_characters() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"Test content";
        let result = file_manager.save_file("file with spaces.txt", content, Some("text/plain"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
    }

    #[test]
    fn test_save_file_with_query_parameters() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"CSS content";
        let result = file_manager.save_file("style.css?v=1.0", content, Some("text/css"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Check that query parameters were sanitized
        let filename = saved_path.file_name().unwrap().to_string_lossy();
        assert!(filename.contains("_"));
    }

    #[test]
    fn test_save_file_with_hash_fragment() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"CSS content";
        let result = file_manager.save_file("style.css#main", content, Some("text/css"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Check that hash fragments were sanitized
        let filename = saved_path.file_name().unwrap().to_string_lossy();
        assert!(filename.contains("_"));
    }

    #[test]
    fn test_save_file_with_ampersands() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"CSS content";
        let result = file_manager.save_file("style.css&param=value", content, Some("text/css"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Check that ampersands were sanitized
        let filename = saved_path.file_name().unwrap().to_string_lossy();
        assert!(filename.contains("_"));
    }

    #[test]
    fn test_save_file_with_equals_sign() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"CSS content";
        let result = file_manager.save_file("style.css=value", content, Some("text/css"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Check that equals signs were sanitized
        let filename = saved_path.file_name().unwrap().to_string_lossy();
        assert!(filename.contains("_"));
    }

    #[test]
    fn test_save_file_with_question_mark() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"CSS content";
        let result = file_manager.save_file("style.css?param=value", content, Some("text/css"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Check that question marks were sanitized
        let filename = saved_path.file_name().unwrap().to_string_lossy();
        assert!(filename.contains("_"));
    }

    #[test]
    fn test_save_file_with_multiple_special_characters() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"CSS content";
        let result = file_manager.save_file("style.css?param=value&other=123#fragment", content, Some("text/css"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Check that all special characters were sanitized
        let filename = saved_path.file_name().unwrap().to_string_lossy();
        assert!(filename.contains("_"));
        assert!(!filename.contains("?"));
        assert!(!filename.contains("&"));
        assert!(!filename.contains("="));
        assert!(!filename.contains("#"));
    }

    #[test]
    fn test_save_file_with_unicode_characters() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"Unicode content";
        let result = file_manager.save_file("file-émojis-🚀.txt", content, Some("text/plain"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Check that unicode characters were sanitized
        let filename = saved_path.file_name().unwrap().to_string_lossy();
        assert!(filename.contains("_"));
    }

    #[test]
    fn test_save_file_with_dot_files() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"Hidden file content";
        let result = file_manager.save_file(".hidden", content, Some("text/plain"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
    }

    #[test]
    fn test_save_file_with_trailing_slash() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"Directory index";
        let result = file_manager.save_file("dir/", content, Some("text/html"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Should create index.html in the directory
        let expected_path = temp_dir.path().join("dir").join("index.html");
        assert!(expected_path.exists());
    }

    #[test]
    fn test_save_file_with_root_path() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"Root content";
        let result = file_manager.save_file("/", content, Some("text/html"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Should create index.html in the root
        let expected_path = temp_dir.path().join("index.html");
        assert!(expected_path.exists());
    }

    #[test]
    fn test_save_file_with_empty_path() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"Empty path content";
        let result = file_manager.save_file("", content, Some("text/html"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Should create index.html in the root
        let expected_path = temp_dir.path().join("index.html");
        assert!(expected_path.exists());
    }

    #[test]
    fn test_save_file_with_nested_subdirectories() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content = b"Nested content";
        let result = file_manager.save_file("a/b/c/d/file.txt", content, Some("text/plain"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Check that all nested directories were created
        let dir_a = temp_dir.path().join("a");
        let dir_b = temp_dir.path().join("a").join("b");
        let dir_c = temp_dir.path().join("a").join("b").join("c");
        let dir_d = temp_dir.path().join("a").join("b").join("c").join("d");
        
        assert!(dir_a.exists() && dir_a.is_dir());
        assert!(dir_b.exists() && dir_b.is_dir());
        assert!(dir_c.exists() && dir_c.is_dir());
        assert!(dir_d.exists() && dir_d.is_dir());
    }

    #[test]
    fn test_save_file_with_existing_file() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content1 = b"First content";
        let content2 = b"Second content";
        
        // Save first file
        let result1 = file_manager.save_file("test.txt", content1, Some("text/plain"));
        assert!(result1.is_ok());
        
        // Overwrite with second file
        let result2 = file_manager.save_file("test.txt", content2, Some("text/plain"));
        assert!(result2.is_ok());
        
        let saved_path = result2.unwrap();
        assert!(saved_path.exists());
        
        // Check that content was overwritten
        let read_content = fs::read(&saved_path).unwrap();
        assert_eq!(read_content, content2);
    }

    #[test]
    fn test_save_file_with_large_content() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let content: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        let result = file_manager.save_file("large.bin", &content, Some("application/octet-stream"));
        assert!(result.is_ok());
        
        let saved_path = result.unwrap();
        assert!(saved_path.exists());
        
        // Check that content was saved correctly
        let read_content = fs::read(&saved_path).unwrap();
        assert_eq!(read_content, content);
    }

    #[test]
    fn test_file_manager_debug() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        let debug_str = format!("{:?}", file_manager);
        assert!(debug_str.contains("FileManager"));
        assert!(debug_str.contains(temp_dir.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn test_file_manager_clone() {
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        let cloned = file_manager.clone();
        
        assert_eq!(file_manager.base_dir, cloned.base_dir);
    }

    #[test]
    fn test_file_manager_partial_eq() {
        let temp_dir = tempdir().unwrap();
        let file_manager1 = FileManager::new(temp_dir.path()).unwrap();
        let file_manager2 = FileManager::new(temp_dir.path()).unwrap();
        
        assert_eq!(file_manager1, file_manager2);
    }

    #[test]
    fn test_file_manager_hash() {
        use std::collections::HashMap;
        
        let temp_dir = tempdir().unwrap();
        let file_manager = FileManager::new(temp_dir.path()).unwrap();
        
        let mut map = HashMap::new();
        map.insert(file_manager.clone(), "value");
        
        assert_eq!(map.get(&file_manager), Some(&"value"));
    }
} 
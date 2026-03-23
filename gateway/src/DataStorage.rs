// DataStorage (Component 3) 
use std::fs::OpenOptions;
use std::sync::{Arc, RwLock}; // Use RwLock for thread-safe file access (allows multiple readers or one writer)
use serde::Serialize;
use crate::common::AggregatedFrame;
use serde_json;
use std::io::{Read, Write, Result, Error};
use std::io::ErrorKind;

/// Core DataStorage struct 
pub struct DataStorage {
    file_path: String,          // Path for Web Server to read
    file: Arc<RwLock<std::fs::File>>, // Thread-safe file handle (prevents race conditions)
}

impl DataStorage {
    /// Create a new DataStorage instance (pre-opens file for performance)
    pub fn new(file_path: &str) -> Result<Self> {
        // Open file with safe configuration
        let file = OpenOptions::new()
            .create(true)          
            .append(true)      
            .read(true)            
            .write(true)           
            .open(file_path)?;

        Ok(Self {
            file_path: file_path.to_string(),
            file: Arc::new(RwLock::new(file)), // Wrap in RwLock for thread safety
        })
    }

    /// Write aggregated frame to file (core requirement)
    
    pub fn write(&self, frame: AggregatedFrame) -> Result<()> {
        // Acquire lock: ensures only one thread writes at a time (prevents data corruption)
        let mut file = self.file.write()
            .map_err(|e| Error::new(std::io::ErrorKind::Other, format!("Lock error: {}", e)))?;

        // Step 1: Serialize frame to JSON (parsable by Web Server)
        let json_str = serde_json::to_string(&frame)
            .map_err(|e| Error::new(std::io::ErrorKind::InvalidData, format!("JSON serialization failed: {}", e)))?;

        // Step 2: Write JSON (one line per frame for readability)
        writeln!(file, "{}", json_str)?;

        // Step 3: Flush to ensure data is written to disk (persistence requirement)
        self.flush_internal(&mut file)?;

        Ok(())
    }

    /// Ensures buffered data is persisted to physical disk (prevents data loss on crash)
    pub fn flush(&self) -> Result<()> {
        let mut file = self.file.write()
            .map_err(|e| Error::new(std::io::ErrorKind::Other, format!("Lock error: {}", e)))?;
        self.flush_internal(&mut file)
    }

    /// Read file content (shared read lock - multiple readers allowed)
    pub fn read_file(&self) -> Result<String> {
        // Acquire shared read lock (multiple threads can read simultaneously)
        let file = self.file.read()
            .map_err(|e| Error::new(ErrorKind::Other, format!("Read lock error (poisoned): {}", e)))?;

        // Read entire file content (JSON lines format)
        let mut content = String::new();
        std::io::Read::read_to_string(&*file, &mut content)?;
        
        Ok(content)
    }

    /// Internal flush helper (syncs to physical disk)
    fn flush_internal(&self, file: &mut std::fs::File) -> Result<()> {
        file.flush()?; // Flush OS buffer to file
        file.sync_all()?; // Sync file data/metadata to physical disk (critical for durability)
        Ok(())
    }

    /// Allows Web Server to locate and read the file
    pub fn path(&self) -> &str {
        &self.file_path
    }
}
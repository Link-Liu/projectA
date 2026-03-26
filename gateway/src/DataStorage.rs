// DataStorage (Component 3) 
use std::fs::OpenOptions;
use std::sync::{Arc, RwLock};
use crate::AggregatedFrame::AggregatedFrame;
use serde_json;
use std::io::{Read, Write, Result, Error};
use std::io::ErrorKind;

/// Core DataStorage struct 
pub struct DataStorage {
    file_path: String,          // Path for Web Server to read
    lock: Arc<RwLock<()>>,
}

impl DataStorage {
    /// Create a new DataStorage instance (pre-opens file for performance)
    pub fn new(file_path: &str) -> Result<Self> {
        // Ensure target file exists
        OpenOptions::new()
            .create(true)          
            .append(true)
            .open(file_path)?;

        Ok(Self {
            file_path: file_path.to_string(),
            lock: Arc::new(RwLock::new(())),
        })
    }

    /// Write aggregated frame to file (core requirement)
    
    pub fn write(&self, frame: AggregatedFrame) -> Result<()> {
        // Acquire lock: ensures only one writer at a time
        let _guard = self.lock.write()
            .map_err(|e| Error::new(std::io::ErrorKind::Other, format!("Lock error: {}", e)))?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;

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
        let _guard = self.lock.write()
            .map_err(|e| Error::new(std::io::ErrorKind::Other, format!("Lock error: {}", e)))?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;

        self.flush_internal(&mut file)
    }

    /// Read file content (shared read lock - multiple readers allowed)
    pub fn read_file(&self) -> Result<String> {
        // Acquire shared read lock (multiple readers can read concurrently)
        let _guard = self.lock.read()
            .map_err(|e| Error::new(ErrorKind::Other, format!("Read lock error (poisoned): {}", e)))?;

        let mut file = OpenOptions::new()
            .read(true)
            .open(&self.file_path)?;

        // Read entire file content (JSON lines format)
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        
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
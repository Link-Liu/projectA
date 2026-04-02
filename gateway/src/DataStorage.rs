// DataStorage (Component 3) 
use std::fs::OpenOptions;
use std::sync::{Arc, RwLock};
use crate::AggregatedFrame::AggregatedFrame;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AggregatedFrame::{AggregatedFrame, SensorInfo};
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn unique_temp_file(name: &str) -> PathBuf {
        // Build a unique file path under the system temp directory.
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_nanos();
        std::env::temp_dir().join(format!("{}_{}_{}.jsonl", name, std::process::id(), nanos))
    }

    fn sample_frame(sensor_id: &str) -> AggregatedFrame {
        AggregatedFrame {
            frame_id: "frame-1".to_string(),
            window_start: SystemTime::now(),
            window_end: SystemTime::now(),
            sensor_info: SensorInfo {
                sensor_id: sensor_id.to_string(),
                total_readings: 5,
                min_value: 1.0,
                max_value: 9.0,
                avg_value: 5.0,
                std_dev: 2.0,
            },
            anomaly_info: None,
        }
    }

    #[test]
    fn test_write_then_read_file_contains_record() {
        let path = unique_temp_file("datastorage_test");
        let storage = DataStorage::new(path.to_str().expect("valid temp path"))
            .expect("failed to create storage");

        storage
            .write(sample_frame("sensor-1"))
            .expect("failed to write frame");

        let content = storage.read_file().expect("failed to read file");

        // Verify that one JSON line exists and includes the expected sensor id.
        assert_eq!(content.lines().count(), 1);
        assert!(content.contains("\"sensor_id\":\"sensor-1\""));

        let _ = std::fs::remove_file(path);
    }
}
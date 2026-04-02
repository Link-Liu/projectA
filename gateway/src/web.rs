use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use std::collections::HashMap;
use crate::DataStorage::DataStorage;
use crate::AggregatedFrame::AggregatedFrame;


type SharedState = Arc<DataStorage>;

pub struct WebServer {
    /// the storage to read the data from
    storage: SharedState,
}
// Component 4: Web Server 
impl WebServer {
    
    pub fn new(storage: Arc<DataStorage>) -> Self {
        Self { storage }
    }

    
    pub async fn serve(self, listener: tokio::net::TcpListener) -> Result<(), std::io::Error> {
        let app = Router::new()
            .route("/latest", get(handle_latest))
            .route("/sensor/:id", get(handle_sensor))
            .route("/stats", get(handle_stats))
            .with_state(Arc::clone(&self.storage));

        let addr = listener.local_addr()?;
        println!("Web server listening on http://{}", addr);

        axum::serve(listener, app).await?;
        Ok(())
    }
}


async fn handle_latest(State(storage): State<SharedState>) -> Json<Vec<AggregatedFrame>> {
    // Use a HashMap to keep only the latest frame for each sensor_id
    let mut latest_frames: HashMap<String, AggregatedFrame> = HashMap::new();
    
    // Safely read the file from the storage
    if let Ok(content) = storage.read_file() {
        // Parse JSON Lines
        for line in content.lines() {
            if let Ok(frame) = serde_json::from_str::<AggregatedFrame>(line) {
                // Since the file is append-only, later entries will overwrite earlier ones,
                // leaving only the most recent frame for each sensor in the map.
                latest_frames.insert(frame.sensor_info.sensor_id.clone(), frame);
            }
        }
    }
    
    // Collect the values from the HashMap into a Vec
    let frames: Vec<AggregatedFrame> = latest_frames.into_values().collect();
    // Return the frames as a JSON array
    Json(frames)
}


async fn handle_sensor(
    State(storage): State<SharedState>,
    Path(sensor_id): Path<String>,
) -> Json<Vec<AggregatedFrame>> {
    let mut frames = Vec::new();
    
    if let Ok(content) = storage.read_file() {
        for line in content.lines() {
            if let Ok(frame) = serde_json::from_str::<AggregatedFrame>(line) {
                // Filter the frames by the sensor_id
                if frame.sensor_info.sensor_id == sensor_id {
                    frames.push(frame);
                }
            }
        }
    }
    
    Json(frames)
}


async fn handle_stats(State(storage): State<SharedState>) -> Json<serde_json::Value> {
    let mut total_records = 0;
    
    if let Ok(content) = storage.read_file() {
        total_records = content.lines().count();
    }
    
    let stats = serde_json::json!({
        "total_records": total_records,
        "status": "running"
    });
    
    Json(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AggregatedFrame::{AggregatedFrame, SensorInfo};
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn unique_temp_file(name: &str) -> PathBuf {
        // Create a unique temp file path for isolated test runs.
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_nanos();
        std::env::temp_dir().join(format!("{}_{}_{}.jsonl", name, std::process::id(), nanos))
    }

    fn sample_frame(frame_id: &str, sensor_id: &str) -> AggregatedFrame {
        AggregatedFrame {
            frame_id: frame_id.to_string(),
            window_start: SystemTime::now(),
            window_end: SystemTime::now(),
            sensor_info: SensorInfo {
                sensor_id: sensor_id.to_string(),
                total_readings: 3,
                min_value: 10.0,
                max_value: 30.0,
                avg_value: 20.0,
                std_dev: 8.0,
            },
            anomaly_info: None,
        }
    }

    #[tokio::test]
    async fn test_handle_stats_counts_all_records() {
        let path = unique_temp_file("web_stats_test");
        let storage = Arc::new(
            DataStorage::new(path.to_str().expect("valid temp path"))
                .expect("failed to create storage"),
        );

        storage
            .write(sample_frame("frame-1", "sensor-a"))
            .expect("failed to write first frame");
        storage
            .write(sample_frame("frame-2", "sensor-b"))
            .expect("failed to write second frame");

        let response = handle_stats(State(Arc::clone(&storage))).await;
        let payload = response.0;

        // Ensure stats reflect the current number of stored lines.
        assert_eq!(payload["total_records"], serde_json::json!(2));
        assert_eq!(payload["status"], serde_json::json!("running"));

        let _ = std::fs::remove_file(path);
    }
}
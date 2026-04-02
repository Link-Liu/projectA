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
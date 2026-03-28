use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use std::collections::HashMap;
use crate::DataStorage::DataStorage;
use crate::AggregatedFrame::AggregatedFrame;

/// Shared state for the web server
type SharedState = Arc<DataStorage>;

/// WebServer struct
pub struct WebServer {
    /// the storage to read the data from
    storage: SharedState,
}
// Component 4: Web Server 
impl WebServer {
    /// Constructs a web server that reads aggregated data from the given storage.
    /// 
    ///  Parameters:
    ///  - storage: the storage to read the data from
    /// 
    ///  Returns:
    ///  - A new WebServer
    /// 
    /// Example:
    /// ```
    /// let web_server = WebServer::new(storage);
    /// 
    pub fn new(storage: Arc<DataStorage>) -> Self {
        Self { storage }
    }

    /// Runs the HTTP server on the provided bound listener until it shuts down.
    /// 
    ///  Parameters:
    ///  - listener: the listener to serve the HTTP server on
    /// 
    ///  Returns:
    ///  - Ok(()) if the server serves successfully
    ///  - Err(std::io::Error) if the server fails to serve
    /// 
    /// Example:
    /// ```
    /// let web_server = WebServer::new(storage);
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

/// `GET /latest`
/// 
/// Returns the most recent aggregated frame for each sensor.
///
///  Parameters:
///  - storage: the storage to read the data from
/// 
///  Returns:
///  - an array containing only the most recent [`AggregatedFrame`] for each sensor.
///  - On read failure, returns an empty JSON array. Malformed lines are skipped without failing the request.
/// 
/// Example:
/// ```
/// let frames = handle_latest(storage).await;
/// ```
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

/// `GET /sensor/:id`
///
///  Parameters:
///  - storage: the storage to read the data from
///  - sensor_id: the ID of the sensor to filter by
/// 
///  Returns:
///  - all aggregated frames whose `sensor_info.sensor_id` equals the path parameter `id`.
///  - On read failure, returns an empty JSON array. Malformed lines are skipped without failing the request.
/// 
/// Example:
/// The path segment is only used for filtering parsed data; it is not used as a filesystem path.
/// On read failure, returns an empty array. Malformed lines are skipped.
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

/// `GET /stats`
///
///  Parameters:
///  - storage: the storage to read the data from
/// 
///  Returns:
///  - a small JSON object with `total_records` (number of lines in the storage file after a successful read)
///  - a fixed `"status": "running"` field.
///  - On read failure, `total_records` is `0`.
/// 
/// Example:
/// and a fixed `"status": "running"` field. On read failure, `total_records` is `0`.
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
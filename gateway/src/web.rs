use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use crate::DataStorage::DataStorage;
use crate::AggregatedFrame::AggregatedFrame;

// 定义我们要在路由间共享的状态
type SharedState = Arc<DataStorage>;

pub struct WebServer {
    storage: SharedState,
}

impl WebServer {
    pub fn new(storage: Arc<DataStorage>) -> Self {
        Self { storage }
    }

    /// 在已绑定的 listener 上提供 HTTP 服务（listener 由 main 中绑定，便于端口占用时提前报错退出）。
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

// GET /latest
// 返回所有最新的聚合帧（简单起见，这里返回文件里的所有数据）
async fn handle_latest(State(storage): State<SharedState>) -> Json<Vec<AggregatedFrame>> {
    let mut frames = Vec::new();
    
    // 安全读取文件 (DataStorage 内部使用了 RwLock)
    if let Ok(content) = storage.read_file() {
        // 解析 JSON Lines
        for line in content.lines() {
            if let Ok(frame) = serde_json::from_str::<AggregatedFrame>(line) {
                frames.push(frame);
            }
        }
    }
    
    // Axum 会自动将 Vec<AggregatedFrame> 序列化为 JSON 响应
    Json(frames)
}

// GET /sensor/:id
// 返回特定传感器的历史数据
async fn handle_sensor(
    State(storage): State<SharedState>,
    Path(sensor_id): Path<String>,
) -> Json<Vec<AggregatedFrame>> {
    let mut frames = Vec::new();
    
    if let Ok(content) = storage.read_file() {
        for line in content.lines() {
            if let Ok(frame) = serde_json::from_str::<AggregatedFrame>(line) {
                // 过滤出匹配特定 sensor_id 的数据
                if frame.sensor_info.sensor_id == sensor_id {
                    frames.push(frame);
                }
            }
        }
    }
    
    Json(frames)
}

// GET /stats
// 返回系统统计信息（例如：总记录数）
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
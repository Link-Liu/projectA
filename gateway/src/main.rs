use dashboard::APP;
use socket2::{Domain, Socket, Type};
use std::net::SocketAddr;
use std::sync::Arc;
use sensor_sim::{
    accelerometer::Accelerometer,
    force_sensor::ForceSensor,
    thermometer::Thermometer,
    traits::Sensor,
};
pub mod buffer;
pub mod DataStorage;
pub mod AggregatedFrame;
pub mod web;
pub mod engine;
use crate::buffer::{SensorBufferManager, SensorKind};

/// Bind with `SO_REUSEADDR` so dev restarts are less likely to hit TIME_WAIT issues.
/// If another process is already listening on the port, binding still fails — end that process first.
async fn bind_web_listener(addr: &str) -> std::io::Result<tokio::net::TcpListener> {
    let addr: SocketAddr = addr
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let sock = Socket::new(domain, Type::STREAM, None)?;
    sock.set_reuse_address(true)?;
    sock.bind(&addr.into())?;
    sock.listen(128)?;
    let std_listener: std::net::TcpListener = sock.into();
    std_listener.set_nonblocking(true)?;
    tokio::net::TcpListener::from_std(std_listener)
}

#[tokio::main]
async fn main() {
    // Initialize 5 sensors: 2 thermometers, 2 accelerometers, 1 force sensor.
    let mut thermo_1 = Thermometer::new("thermo-1".to_string(), 10);
    let mut thermo_2 = Thermometer::new("thermo-2".to_string(), 10);
    let mut accel_1 = Accelerometer::new("accel-1".to_string(), 20);
    let mut accel_2 = Accelerometer::new("accel-2".to_string(), 20);
    let mut force_1 = ForceSensor::new("force-1".to_string(), 15);

    // Start all sensors (each spawns its own background thread).
    thermo_1.start();
    thermo_2.start();
    accel_1.start();
    accel_2.start();
    force_1.start();
    let mut buffer_mgr = SensorBufferManager::new(10000);
    buffer_mgr.register_sensor(thermo_1, SensorKind::ThermoReading);
    buffer_mgr.register_sensor(thermo_2, SensorKind::ThermoReading);
    buffer_mgr.register_sensor(accel_1, SensorKind::AccelReading);
    buffer_mgr.register_sensor(accel_2, SensorKind::AccelReading);
    buffer_mgr.register_sensor(force_1, SensorKind::ForceReading);

    let buffer_mgr = Arc::new(buffer_mgr);

    // 1. Initialize DataStorage
    // Ensure the data directory exists before creating the file
    if let Some(parent) = std::path::Path::new("data/aggregated.json").parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let storage = Arc::new(crate::DataStorage::DataStorage::new("data/aggregated.json").unwrap());

    // 2. Start AggregationEngine
    let mut engine = crate::engine::AggregationEngine::new(crate::engine::EngineConfiguration {
        window_duration: std::time::Duration::from_secs(1),
        num_workers: 2,
        anomaly_threshold: 3.0,
    });
    engine.start(Arc::clone(&buffer_mgr), Arc::clone(&storage));

    // 3. Web server: bind before Hotaru so AddrInUse fails fast with a clear message
    const WEB_ADDR: &str = "127.0.0.1:8080";
    let web_listener = match bind_web_listener(WEB_ADDR).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "无法绑定 Web 服务 {WEB_ADDR}: {e}\n\
                 仍有进程占用 8080（不一定是刚才那个 PID）。请查看并结束:\n  \
                 lsof -i :8080\n  \
                 kill <PID>\n\
                 或一键: kill $(lsof -t -i:8080) 2>/dev/null\n"
            );
            std::process::exit(1);
        }
    };

    let web_storage = Arc::clone(&storage);
    let web_server = crate::web::WebServer::new(web_storage);
    tokio::spawn(async move {
        if let Err(e) = web_server.serve(web_listener).await {
            eprintln!("Web server stopped: {e}");
        }
    });

    APP.clone().run().await;
}

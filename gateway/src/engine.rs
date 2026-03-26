use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle}; 
use std::time::{Duration, Instant};
use std::collections::HashMap;

use crate::buffer::{SensorBufferManager, SensorKind}; 
use crate::AggregatedFrame::{AggregatedFrame, SensorInfo};
use crate::DataStorage::DataStorage;

struct SensorStats {
    count: usize,
    mean: f64,
    m2: f64,
    min: f64,
    max: f64,
}

impl SensorStats {
    fn new() -> Self {
        Self {
            count: 0,
            mean: 0.0,
            m2: 0.0,
            min: f64::MAX,
            max: f64::MIN,
        }
    }

    fn update(&mut self, val: f64) {
        self.count += 1;
        // find the delta between the new value and the current mean
        let delta = val - self.mean;
        // update the mean
        self.mean += delta / self.count as f64;
        // find the delta between the new value and the new mean
        let delta2 = val - self.mean;
        // update sum of squared differences
        self.m2 += delta * delta2;
        if val < self.min 
            { self.min = val; }
        if val > self.max 
            { self.max = val; }
    }

    fn get_std_dev(&self) -> f64 {
        // calculates the standard deviation
        if self.count < 2 
            { 0.0 } 
        else 
            { (self.m2 / self.count as f64).sqrt() }
    }
}

pub struct EngineConfiguration 
{
    pub window_duration: Duration, //窗口时长 比如每五分钟计算一次平均值
    pub num_workers: usize, //工作线程数量
    pub anomaly_threshold: f64, //异常检测阈值
    
}


pub struct AggregationEngine 
{
    config: EngineConfiguration,
    workers: Vec<JoinHandle<()>>,
    shutdown_flag: Arc<AtomicBool>,
}

impl AggregationEngine {
    pub fn new(config: EngineConfiguration) -> Self {
        Self {
            config,
            workers: Vec::new(),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(
        &mut self, 
        buffer_manager: Arc<SensorBufferManager>, 
        // storage: Arc<DataStorage> // 如果还没定义 DataStorage，先注释掉
    ) {
        let num_workers = self.config.num_workers;

        for i in 0..num_workers {
            let shutdown = Arc::clone(&self.shutdown_flag);
            let buffer = Arc::clone(&buffer_manager);
            let threshold = self.config.anomaly_threshold;
            let window_dur = self.config.window_duration;

            let handle = thread::spawn(move || {
                let mut sensor_map: HashMap<String, SensorStats> = HashMap::new();
                let mut window_start = Instant::now();

                while !shutdown.load(Ordering::SeqCst) {
                    // 修正：将逻辑放入 if let 内部，确保 id 和 val 可用
                    if let Some(sensor_data) = buffer.pop_with_timeout(Duration::from_millis(100)) {
                        let (id, val) = match sensor_data.kind {
                            SensorKind::ThermoReading(t) => (sensor_data.id, t.temperature_celsius as f64),
                            SensorKind::AccelReading(a) => {
                                let mag = (a.acceleration_x * a.acceleration_x + a.acceleration_y * a.acceleration_y  + a.acceleration_z * a.acceleration_z).sqrt();
                                (sensor_data.id, mag as f64)
                            },
                            SensorKind::ForceReading(f) => {
                                let mag = (f.force_x * f.force_x  + f.force_y * f.force_y  + f.force_z * f.force_z).sqrt();
                                (sensor_data.id, mag as f64)
                            }
                        };

                        // 这里的逻辑必须在 if let 的 {} 里面
                        let stats: &mut SensorStats = sensor_map.entry(id.clone()).or_insert_with(SensorStats::new);
                        stats.update(val);

                        // 异常检测
                        let std_dev = stats.get_std_dev();
                        if stats.count > 10 && (val - stats.mean).abs() > threshold * std_dev {
                            println!("Anomaly detected for sensor {}: value={}, mean={}, std_dev={}", id, val, stats.mean, std_dev);
                        }
                    } // if let 结束

                    // 检查窗口是否结束
                    if window_start.elapsed() >= window_dur {
                        let now = std::time::SystemTime::now();
                        let start_time = now - window_dur;
                        for (id, stats) in &sensor_map
                        {
                            let sensor_info = SensorInfo
                            {
                                sensor_id: id.clone(),
                                total_readings: stats.count as u32,
                                min_value: stats.min,
                                max_value: stats.max,
                                avg_value: stats.mean,
                                std_dev: stats.get_std_dev(),
                            };
                            let frame = AggregatedFrame
                            {
                                frame_id: format!("{}-{}", id, now.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()),
                                window_start: start_time,
                                window_end: now,
                                sensor_info,
                                anomaly_info: None, 
                            };
                            storage_out.write_frame(frame);
                        }
                    sensor_map.clear();
                    window_start = Instant::now();

                    }
                }
                println!("Worker {}: Shutting down gracefully", i);
            });

            self.workers.push(handle);
        }
    }

    pub fn shutdown(self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
        for handle in self.workers {
            let _ = handle.join();
        }
    }
}
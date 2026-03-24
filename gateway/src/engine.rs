use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle}; 
use std::time::{Duration, Instant};
use std::collections::HashMap;

use crate::buffer::{SensorBufferManager, SensorData}; 

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
        storage: Arc<DataStorage> // C3 待实现
    ) {
        let num_workers = self.config.num_workers;

        for i in 0..num_workers {
            let shutdown = Arc::clone(&self.shutdown_flag);
            let buffer = Arc::clone(&buffer_manager);
            // let storage_out = Arc::clone(&storage);
            let threshold = self.config.anomaly_threshold;
            let window_dur = self.config.window_duration;

            let handle = thread::spawn(move || {
                // 每个线程维护一个状态表，Key 是传感器 ID，Value 是统计状态
                let mut sensor_map: HashMap<String, SensorStats> = HashMap::new();
                let mut window_start = Instant::now();

                // 直到收到关机指令
                while !shutdown.load(Ordering::SeqCst) {
                    
                    // --- 步骤 1: 获取数据 ---
                    if let Some(sensor_data) = buffer.pop_with_timeout(Duration::from_millis(100)) {
                        // 识别 Sensor ID 和数值
                        let (id, val) = match sensor_data {
                            SensorData::ThermoReading(t) => (t.id().to_string(), t.temperature() as f64),
                            SensorData::AccelReading(a) => {
                                let mag = (a.x*a.x + a.y*a.y + a.z*a.z).sqrt();
                                (a.id().to_string(), mag as f64)
                            },
                            SensorData::ForceReading(f) => {
                                let mag = (f.x*f.x + f.y*f.y + f.z*f.z).sqrt();
                                (f.id().to_string(), mag as f64)
                            }
                        };
                    }

                        let stats = sensor_map.entry(id.clone()).or_insert_with(SensorStats::new);
                        stats.update(val);

                        // anomaly detection
                        let std_dev = stats.get_std_dev();
                        if stats.count > 10 && (val - stats.mean).abs() > threshold * std_dev {
                            println!("Anomaly detected for sensor {}: value={}, mean={}, std_dev={}", id, val, stats.mean, std_dev);
                        }
                    

                    // check if window duration ended
                    if window_start.elapsed() >= window_dur {
                        if !sensor_map.is_empty() {
                            println!("Worker {}: Window ended, processing {} sensors", i, sensor_map.len());
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
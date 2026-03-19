use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Condvar};
use std::thread::JoinHandle;
use std::time::Duration;
use sensor_sim::accelerometer::AccelReading;
use sensor_sim::force_sensor::ForceReading;
use sensor_sim::thermometer::ThermoReading;
use sensor_sim::traits::Sensor;

pub enum SensorData {
    ThermoReading(ThermoReading),
    AccelReading(AccelReading),
    ForceReading(ForceReading),
}

pub struct SensorBufferManager {
    capacity: usize,
    buffer: Arc<Mutex<VecDeque<SensorData>>>,
    readers: Vec<JoinHandle<()>>,
    stop_flag: Arc<Mutex<bool>>,
    has_data: Arc<Condvar>
}

impl SensorBufferManager {
    /// Create a manager with buffer capacity
    pub fn new(capacity: usize) -> Self {
        let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(capacity)));
        SensorBufferManager {
            capacity : capacity,
            buffer : buffer,
            readers : Vec::new(),
            stop_flag : Arc::new(Mutex::new(false)),
            has_data : Arc::new(Condvar::new())
        }
    }

    ///  Register a Sensor (spawns reader thread)
    pub fn register_sensor<S, F>(&mut self, sensor: S, converter: F ) 
    where 
    S: Sensor + Send + 'static,
    S::SensorReading: Send + 'static,
    F: Fn(S::SensorReading) -> SensorData + Send + Sync + 'static,
    {
        let shared_buffer = Arc::clone(&self.buffer);
        let stop_flag = Arc::clone(&self.stop_flag);
        let  has_data = Arc::clone(&self.has_data);
        let handle = std::thread::spawn(move || {
            let sensor = sensor;
            while !*stop_flag.lock().unwrap() {
                if let Some(content) = sensor.read() {
                    let data = converter(content);
                    let mut shared_buffer = shared_buffer.lock().unwrap();
                    if shared_buffer.len() < shared_buffer.capacity() {
                        shared_buffer.push_back(data);
                        has_data.notify_one();
                    }
                    else {
                        shared_buffer.pop_front();
                        shared_buffer.push_back(data);
                        has_data.notify_one();
                        println!("Attention! Buffer is full, overwriting oldest data");
                    }
                }
                else {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        });
        self.readers.push(handle);
    }

    /// Pop reading for processing (blocking)
    pub fn pop_blocking(&self) -> SensorData {
        let mut shared_buffer = self.buffer.lock().unwrap();
        while shared_buffer.is_empty() {
            shared_buffer = self.has_data.wait(shared_buffer).unwrap();
        }
        return shared_buffer.pop_front().unwrap()
    }
    /// Pop with timeout
    pub fn pop_with_timeout(&self, duration: Duration) -> Option<SensorData> {
           let mut shared_buffer = self.buffer.lock().unwrap();
           
           if shared_buffer.is_empty() {
                while shared_buffer.is_empty() {
                    let (new_shared_buffer, result) = self.has_data.wait_timeout(shared_buffer, duration).unwrap();
                    shared_buffer = new_shared_buffer;
                    if result.timed_out() {
                        return None;
                    }
                    else {
                        return shared_buffer.pop_front();
                    }
                }
                return None;
           }
           else {
                return shared_buffer.pop_front()
           }
    }
    /// Get buffer utilization statistics
    pub fn get_stats(&self) -> f32 {
        let shared_buffer = self.buffer.lock().unwrap();
        let count = shared_buffer.len();
        let capacity = self.capacity;
        count as f32 / capacity as f32
    }
    /// Shutdown all reader threads
    pub fn shutdown(&mut self) {
        let mut stop_flag = self.stop_flag.lock().unwrap();
        *stop_flag = true;
        while !self.readers.is_empty() {
            let handle = self.readers.pop().unwrap();
            let _ = handle.join().unwrap();
        }
    }
}
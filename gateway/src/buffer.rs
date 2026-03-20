use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Condvar};
use std::thread::JoinHandle;
use std::time::Duration;
use sensor_sim::accelerometer::AccelReading;
use sensor_sim::force_sensor::ForceReading;
use sensor_sim::thermometer::ThermoReading;
use sensor_sim::traits::Sensor;

/// SensorData is the data type for the sensor data
pub enum SensorData {
    ThermoReading(ThermoReading),
    AccelReading(AccelReading),
    ForceReading(ForceReading),
}

/// used to store the buffer statistics
pub struct BufferStats {
    pub utilization: f32,
    pub overwrite_count: usize,
    pub write_rate: f32,
    pub pop_rate: f32
}

/// Component 1: Buffer Management
pub struct SensorBufferManager {
    capacity: usize, // how many data we have
    buffer: Arc<Mutex<VecDeque<SensorData>>>, // the buffer that stores the data
    readers: Vec<JoinHandle<()>>, // a vector of handles to the reader threads
    stop_flag: Arc<Mutex<bool>>, // a flag to stop the buffer, when true, stop
    has_data: Arc<Condvar>, // a condition variable to wait for data
    overwrite_count: Arc<Mutex<usize>>, // a counter to count the number of overwrites
    write_count: Arc<Mutex<usize>>, // a counter to count the number of writes
    pop_count: Arc<Mutex<usize>>, // a counter to count the number of pops
    start_time: std::time::Instant // the start time of the buffer
}

impl SensorBufferManager {
    /// Create a manager with buffer capacity
    /// 
    ///  Parameters:
    ///  - capacity: the capacity of the buffer
    /// 
    ///  Returns:
    ///  - A new SensorBufferManager
    /// 
    /// Example:
    /// ```
    /// let buffer_mgr = SensorBufferManager::new(10000);
    /// ```
    pub fn new(capacity: usize) -> Self {
        let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(capacity)));
        SensorBufferManager {
            capacity : capacity,
            buffer : buffer,
            readers : Vec::new(),
            stop_flag : Arc::new(Mutex::new(false)),
            has_data : Arc::new(Condvar::new()),
            overwrite_count : Arc::new(Mutex::new(0)),
            write_count : Arc::new(Mutex::new(0)),
            pop_count : Arc::new(Mutex::new(0)),
            start_time : std::time::Instant::now()
        }
    }

    ///  Register a Sensor (spawns reader thread)
    ///  Parameters:
    ///  - sensor: the sensor to register
    ///  - converter: a function to convert the sensor data to SensorData
    /// 
    ///  Returns:
    ///  - None
    /// 
    /// Example:
    /// ```
    /// let buffer_mgr = SensorBufferManager::new(10000);
    /// buffer_mgr.register_sensor(sensor, converter);
    /// ```
    pub fn register_sensor<S, F>(&mut self, sensor: S, converter: F ) 
    where 
    S: Sensor + Send + 'static,
    S::SensorReading: Send + 'static,
    F: Fn(S::SensorReading) -> SensorData + Send + Sync + 'static,
    {
        // clone the shared resources
        let shared_buffer = Arc::clone(&self.buffer);
        let stop_flag = Arc::clone(&self.stop_flag);
        let  has_data = Arc::clone(&self.has_data);
        let overwrite_count = Arc::clone(&self.overwrite_count);
        let write_count = Arc::clone(&self.write_count);
        // spawn a new thread to read the sensor data
        let handle = std::thread::spawn(move || {
            // clone the sensor
            let sensor = sensor;
            // while the buffer is not stopped
            while !*stop_flag.lock().unwrap() {
                // while the sensor has data, we read all the data from the sensor
                while let Some(content) = sensor.read() {
                    let data = converter(content);
                    let mut shared_buffer = shared_buffer.lock().unwrap();
                    if shared_buffer.len() < shared_buffer.capacity() {
                        // store the data and increase the write count
                        shared_buffer.push_back(data);
                        let mut write_count = write_count.lock().unwrap();
                        *write_count += 1;
                        has_data.notify_one();
                    }
                    else {
                        // data loss here, we alarm the user
                        shared_buffer.pop_front();
                        shared_buffer.push_back(data);
                        // increase the overwrite count and the write count
                        // we overwrite the oldest data
                        let mut overwrite_count = overwrite_count.lock().unwrap();
                        *overwrite_count += 1;
                        let mut write_count = write_count.lock().unwrap();
                        *write_count += 1;
                        has_data.notify_one();
                        println!("Attention! Buffer is full, overwriting oldest data");
                    }
                }
                std::thread::sleep(Duration::from_millis(10));

            }
        });
        self.readers.push(handle);
    }

    /// Pop reading for processing (blocking)
    /// 
    ///  Parameters:
    ///  - None
    /// 
    ///  Returns:
    ///  - The data from the buffer
    /// 
    /// Example:
    /// ```
    /// let data = buffer_mgr.pop_blocking();
    /// ```
    pub fn pop_blocking(&self) -> SensorData {
        let mut shared_buffer = self.buffer.lock().unwrap();
        // while the buffer is empty, we wait for the data
        while shared_buffer.is_empty() {
            shared_buffer = self.has_data.wait(shared_buffer).unwrap();
        }
        let mut pop_count = self.pop_count.lock().unwrap();
        *pop_count += 1;
        return shared_buffer.pop_front().unwrap()
    }
    /// Pop with timeout
    /// 
    ///  Parameters:
    ///  - duration: the timeout duration
    /// 
    ///  Returns:
    ///  - The data from the buffer
    /// 
    /// Example:
    /// ```
    /// let data = buffer_mgr.pop_with_timeout(Duration::from_millis(100));
    /// ```
    pub fn pop_with_timeout(&self, duration: Duration) -> Option<SensorData> {
           let mut shared_buffer = self.buffer.lock().unwrap();
           // while the buffer is empty, we wait for the data
           if shared_buffer.is_empty() {
                while shared_buffer.is_empty() {
                    let (new_shared_buffer, result) = self.has_data.wait_timeout(shared_buffer, duration).unwrap();
                    shared_buffer = new_shared_buffer;
                    if result.timed_out() {
                        // timeout, we return None
                        return None;
                    }
                    else {
                        // we have data, we pop the data and return it
                        let mut pop_count = self.pop_count.lock().unwrap();
                        *pop_count += 1;
                        return shared_buffer.pop_front();

                    }
                }
                return None;
           }
           else {
            // pop the data and increase the pop count
            let mut pop_count = self.pop_count.lock().unwrap();
            *pop_count += 1;
            return shared_buffer.pop_front();
           }
    }
    
    /// Get buffer utilization statistics
    /// 
    ///  Parameters:
    ///  - None
    /// 
    ///  Returns:
    ///  - The buffer statistics
    /// 
    /// Example:
    /// ```
    /// let stats = buffer_mgr.get_stats();
    /// ```
    pub fn get_stats(&self) -> BufferStats {
        let shared_buffer = self.buffer.lock().unwrap();
        let count = shared_buffer.len();
        let capacity = self.capacity;
        let utilization = count as f32 / capacity as f32;
        let overwrite_count = self.overwrite_count.lock().unwrap();
        let write_count = self.write_count.lock().unwrap();
        let pop_count = self.pop_count.lock().unwrap();
        let write_rate = *write_count as f32 / (std::time::Instant::now() - self.start_time).as_secs() as f32;
        let pop_rate = *pop_count as f32 / (std::time::Instant::now() - self.start_time).as_secs() as f32;
        return BufferStats {
            utilization : utilization,
            overwrite_count : *overwrite_count,
            write_rate : write_rate,
            pop_rate : pop_rate
        };

    }
    
    /// Shutdown all reader threads
    /// 
    ///  Parameters:
    ///  - None
    /// 
    ///  Returns:
    ///  - None
    /// 
    /// Example:
    /// ```
    /// buffer_mgr.shutdown();
    /// ```
    pub fn shutdown(&mut self) {
        // we set the stop flag to true, use local variable to avoid dead lock
        {
            let mut stop_flag = self.stop_flag.lock().unwrap();
            *stop_flag = true;
        }
        while !self.readers.is_empty() {
            let handle = self.readers.pop().unwrap();
            let _ = handle.join().unwrap();
        }
    }
}
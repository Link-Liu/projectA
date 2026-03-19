use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

use super::traits::Sensor;
use os_lib::queue::*;

#[derive(Clone, Copy, Debug)]
pub struct ThermoReading {
    pub temperature_celsius: f32,
}

const MAX_QUEUE_SIZE: usize = 128;

pub struct Thermometer {
    id: String,
    rate_per_sec: u32,
    queue: Box<RWRoundQueue<ThermoReading>>,
    reader: QueueReader<ThermoReading>,
    writer: Option<QueueWriter<ThermoReading>>,
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<QueueWriter<ThermoReading>>>,
}

impl Thermometer {
    pub fn start_thread(&mut self) {
        // Test whether handle already exists
        if self.handle.is_some() {
            return;
        }

        self.running.store(true, Ordering::Relaxed);

        let mut writer = self.writer.take().expect("start called twice");
        let rate_per_sec = self.rate_per_sec;
        let running = Arc::clone(&self.running);

        // Implementation for starting data generation thread
        self.handle = Some(std::thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                // Simulate temperature reading generation
                let reading = ThermoReading {
                    temperature_celsius: 20.0 + rand::random::<f32>() * 10.0,
                };

                // Write reading to the queue
                unsafe {
                    writer.write(reading);
                }

                // Sleep according to rate_per_sec
                std::thread::sleep(std::time::Duration::from_millis(1000 / rate_per_sec as u64));
            } 
            return writer; 
        }));
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let writer = handle.join().expect("thread panicked");
            self.writer = Some(writer);
        }
    }
}

impl Sensor for Thermometer {
    type SensorReading = ThermoReading;

    /// Create a new mock sensor with ID and generation rate
    fn new(id: String, rate_per_sec: u32) -> Self {
        // Fix: keep the queue at a stable address before splitting. Previously we
        // split and then moved the queue into the struct, which invalidated the
        // raw pointers held by reader/writer (use-after-move/UB).
        let mut queue = Box::new(RWRoundQueue::new(MAX_QUEUE_SIZE).unwrap());
        let (reader, writer) = unsafe { queue.as_mut().split() };

        Thermometer {
            id,
            rate_per_sec,
            queue,
            reader,
            writer: Some(writer),
            running: Arc::new(AtomicBool::new(true)),
            handle: None,
        }
    }

    /// Create a new mock sensor with ID and generation rate
    fn start(&mut self) {
        self.start_thread();
    }

    /// Read one reading from the sensor's buffer
    /// Returns None if buffer is empty
    fn read(&self) -> Option<Self::SensorReading> {
        self.reader.read()
    }

    /// Get number of unread items in sensor's buffer
    /// If this reaches the upper limit, data loss occurs!
    fn available(&self) -> usize {
        self.reader.len()
    }

    /// Get sensor identifier
    fn id(&self) -> String {
        self.id.clone()
    }

    /// Stop data generation
    fn stop(&mut self) {
        Thermometer::stop(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::Sensor;
    use std::time::Duration;

    struct SensorGuard {
        sensor: Thermometer,
    }

    impl SensorGuard {
        fn new(id: &str, rate_per_sec: u32) -> Self {
            Self {
                sensor: Thermometer::new(id.to_string(), rate_per_sec),
            }
        }
    }

    impl Drop for SensorGuard {
        fn drop(&mut self) {
            self.sensor.stop();
        }
    }

    fn wait_for_reading(sensor: &Thermometer, timeout_ms: u64) -> Option<ThermoReading> {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_millis(timeout_ms) {
            if let Some(reading) = sensor.read() {
                return Some(reading);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }

    #[test]
    fn start_produces_readings() {
        let mut guard = SensorGuard::new("t-1", 200);
        guard.sensor.start();

        let reading = wait_for_reading(&guard.sensor, 500);
        assert!(reading.is_some());
    }

    #[test]
    fn stop_halts_and_restart_resumes() {
        let mut guard = SensorGuard::new("t-2", 200);
        guard.sensor.start();

        assert!(wait_for_reading(&guard.sensor, 500).is_some());

        guard.sensor.stop();

        while guard.sensor.read().is_some() {}
        std::thread::sleep(Duration::from_millis(50));
        assert!(guard.sensor.read().is_none());

        guard.sensor.start();
        assert!(wait_for_reading(&guard.sensor, 500).is_some());
    }

    #[test]
    fn available_matches_reader_len() {
        let mut guard = SensorGuard::new("t-3", 200);
        guard.sensor.start();

        let _ = wait_for_reading(&guard.sensor, 500);
        assert_eq!(guard.sensor.available(), guard.sensor.reader.len());
    }
}

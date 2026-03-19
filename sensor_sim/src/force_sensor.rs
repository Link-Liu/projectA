use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
};

use crate::traits::Sensor;
use os_lib::queue::*;

#[derive(Clone, Copy, Debug)]
pub struct ForceReading {
    pub force_x: f32,
    pub force_y: f32,
    pub force_z: f32,
}

const MAX_QUEUE_SIZE: usize = 128;

pub struct ForceSensor {
    id: String,
    rate_per_sec: u32,
    queue: Box<RWRoundQueue<ForceReading>>,
    reader: QueueReader<ForceReading>,
    writer: Option<QueueWriter<ForceReading>>,
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<QueueWriter<ForceReading>>>,
}

impl ForceSensor {
    pub fn start_thread(&mut self) {
        if self.handle.is_some() {
            return;
        }

        self.running.store(true, Ordering::Relaxed);

        let mut writer = self.writer.take().expect("start called twice");
        let rate_per_sec = self.rate_per_sec;
        let running = Arc::clone(&self.running);

        self.handle = Some(std::thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                let reading = ForceReading {
                    force_x: rand::random::<f32>() * 100.0,
                    force_y: rand::random::<f32>() * 100.0,
                    force_z: rand::random::<f32>() * 100.0,
                };

                unsafe {
                    writer.write(reading);
                }

                std::thread::sleep(std::time::Duration::from_millis(
                    1000 / rate_per_sec as u64,
                ));
            }
            writer
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

impl Sensor for ForceSensor {
    type SensorReading = ForceReading;

    fn new(id: String, rate_per_sec: u32) -> Self {
        let mut queue = Box::new(RWRoundQueue::new(MAX_QUEUE_SIZE).unwrap());
        let (reader, writer) = unsafe { queue.as_mut().split() };

        ForceSensor {
            id,
            rate_per_sec,
            queue,
            reader,
            writer: Some(writer),
            running: Arc::new(AtomicBool::new(true)),
            handle: None,
        }
    }

    fn start(&mut self) {
        self.start_thread();
    }

    fn read(&self) -> Option<Self::SensorReading> {
        self.reader.read()
    }

    fn available(&self) -> usize {
        self.reader.len()
    }

    fn id(&self) -> String {
        self.id.clone()
    }

    fn stop(&mut self) {
        ForceSensor::stop(self);
    }
}

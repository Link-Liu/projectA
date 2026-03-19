use dashboard::APP;
use sensor_sim::{
    accelerometer::Accelerometer,
    force_sensor::ForceSensor,
    thermometer::Thermometer,
    traits::Sensor,
};
pub mod buffer;
use crate::buffer::{SensorBufferManager, SensorData};

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
    let mut buffer_mgr = SensorBufferManager::new(10);
    buffer_mgr.register_sensor(thermo_1, SensorData::ThermoReading);
    buffer_mgr.register_sensor(thermo_2, SensorData::ThermoReading);
    buffer_mgr.register_sensor(accel_1, SensorData::AccelReading);
    buffer_mgr.register_sensor(accel_2, SensorData::AccelReading);
    buffer_mgr.register_sensor(force_1, SensorData::ForceReading);

    // Students are expected to implement using pipe & process instead of threads. 
    // The thread implementation is provided as a idea to let you know what you may do 
    let io_handle = std::thread::spawn(move || {
        // Example gateway loop:
        // - Read fresh data from each sensor.
        // - Write that data to per-sensor files (students implement file I/O).
        // - Later, read those files back so the gateway can process them.
        loop {
            if let Some(reading) = buffer_mgr.pop_with_timeout(std::time::Duration::from_millis(100)) {
                // TODO(student): write `reading` to a file like "data/thermo-1.txt".
                // TODO(student): read from "data/thermo-1.txt" here if gateway consumes files.
                let _ = reading;
            }
            // if let Some(reading) = thermo_2.read() {
            //     // TODO(student): write `reading` to a file like "data/thermo-2.txt".
            //     // TODO(student): read from "data/thermo-2.txt" here if gateway consumes files.
            //     let _ = reading;
            // }
            // if let Some(reading) = accel_1.read() {
            //     // TODO(student): write `reading` to a file like "data/accel-1.txt".
            //     // TODO(student): read from "data/accel-1.txt" here if gateway consumes files.
            //     let _ = reading;
            // }
            // if let Some(reading) = accel_2.read() {
            //     // TODO(student): write `reading` to a file like "data/accel-2.txt".
            //     // TODO(student): read from "data/accel-2.txt" here if gateway consumes files.
            //     let _ = reading;
            // }
            // if let Some(reading) = force_1.read() {
            //     // TODO(student): write `reading` to a file like "data/force-1.txt".
            //     // TODO(student): read from "data/force-1.txt" here if gateway consumes files.
            //     let _ = reading;
            // }

            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });

    APP.clone().run().await;

    let _ = io_handle.join(); 
}

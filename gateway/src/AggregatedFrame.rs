//This file is the final output of the Component 2(Aggregation Engine), and also the Input of the Component 3(DataStorage)

// Import Serialize trait from the serde crate( Allows structs to be converted to JSON format)
use serde::Serialize;

// Import SystemTime 
// Use: Records timestamps 
use std::time::SystemTime;

//Data structure for sensor statistical information
#[derive(Serialize, Debug)]
pub struct SensorInfo {
    // pub = Public (accessible by other modules from other components)
    pub sensor_id: String,       // sensor ID 
    pub total_readings: u32,     // Total number of sensor readings in 1 second
    pub min_value: f64,          // Minimum value of readings in the window
    pub max_value: f64,          // Maximum value of readings in the window
    pub avg_value: f64,          // Average value of readings in the window
    pub std_dev: f64,            // Standard deviation 
}

#[derive(Serialize, Debug)]
pub struct AnomalyInfo {
    pub sensor_id: String,       // ID of the sensor with anomalies
    pub anomaly_type: String,    // Type of anomaly (e.g., "sudden value spike")
    pub anomaly_value: f64,      // The abnormal value detected
    pub description: String,     // Explanation of why this value is abnormal
}


// Core data structure: AggregatedFrame
// Output of Component 2, Input of Component 3

#[derive(Serialize, Debug)]
pub struct AggregatedFrame {
    pub frame_id: String,               //frame ID 
    pub window_start: SystemTime,       // Start time 
    pub window_end: SystemTime,         // End time 
    pub sensor_info: SensorInfo,        // Statistical data (reuses SensorInfo struct)
    pub anomaly_info: Option<AnomalyInfo>, // Option = "optional value" ('Some' if has anomal value, 'None' if no anomal value)
}

pub trait Sensor { 
    type SensorReading; 

    /// Create a new mock sensor with ID and generation rate 
    fn new(id: String , rate_per_sec : u32) -> Self; 

    /// Create a new mock sensor with ID and generation rate
    fn start (&mut self); 

    /// Read one reading from the sensor's buffer
    /// Returns None if buffer is empty
    fn read(&self) -> Option<Self::SensorReading>;

    /// Get number of unread items in sensor's buffer
    /// If this reaches the upper limit, data loss occurs!
    fn available(&self) -> usize;

    /// Get sensor identifier
    fn id(&self) -> String;

    /// Stop data generation
    fn stop(&mut self);
} 

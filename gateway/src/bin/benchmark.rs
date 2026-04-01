use std::sync::Arc;
use std::time::{Duration, Instant};
use serde::Serialize;


use sysinfo::System; 


use gateway::buffer::{SensorBufferManager, SensorKind};
use gateway::engine::{AggregationEngine, EngineConfiguration};
use gateway::DataStorage::DataStorage;

// --- 3. 传感器模拟器引用 ---
use sensor_sim::{
    accelerometer::Accelerometer,
    force_sensor::ForceSensor,
    thermometer::Thermometer,
    traits::Sensor,
};


#[derive(Serialize)]
struct BenchmarkResult {
    sensor_count: usize,
    duration_time: u64,
    throughput_write_rate: f32,
    throughput_pop_rate: f32,
    buffer_utilization: f32,
    data_loss_count: usize,
    avg_cpu_usage_percent: f32,
    memory_usage_mb: f64,
}

#[tokio::main]
async fn main()
{
    println!("Starting benchmark");

    // test with different sensor counts
    let sensor_scales = vec![5, 20, 50, 100];
    let test_duration = Duration::from_secs(15); // each test runs for 15 seconds
    let mut results = Vec::new();
    let mut wtr = csv::Writer::from_path("benchmark_results.csv").unwrap();

    for count in sensor_scales
    {
        println!("\n========================================");
        println!("Testing with {} sensors", count);
        let result = run_test_scenario(count, test_duration).await;
        wtr.serialize(&result).unwrap();
        results.push(result);
    }

    wtr.flush().unwrap();
    println!("\nBenchmark completed. Results saved to benchmark_results.csv");
}

async fn run_test_scenario(sensor_count: usize, duration: Duration) -> BenchmarkResult
{
    let mut sys = System::new_all();
    sys.refresh_all();
    // Initialize Buffer Manager
    let mut buffer_mgr = SensorBufferManager::new(10000);
    let rate;
    if sensor_count >= 50
    {
        rate = 50;
    }
    else
    {
        rate = 20;
    }

    for i in 0..sensor_count
    {
        if i % 3 == 0
        {
            let mut s = Thermometer::new(format!("thermo-{}", i), rate);
            s.start();
            buffer_mgr.register_sensor(s, SensorKind::ThermoReading);
        }
        else if i % 3 == 1
        {
            let mut s = Accelerometer::new(format!("accel-{}", i), rate);
            s.start();
            buffer_mgr.register_sensor(s, SensorKind::AccelReading);
        }
        else
        {
            let mut s = ForceSensor::new(format!("force-{}", i), rate);
            s.start();
            buffer_mgr.register_sensor(s, SensorKind::ForceReading);
        }
    }

    let buffer_mgr = Arc::new(buffer_mgr);
    
    // Initialize DataStorage and Engine
    let storage_path = format!("data/benchmark_agg_{}.json", sensor_count); 
    let storage = Arc::new(DataStorage::new(&storage_path).expect("cannot create storage"));

    let mut engine = AggregationEngine::new(EngineConfiguration
        {
            window_duration: Duration::from_secs(1),
            num_workers: 4,
            anomaly_threshold: 3.0,
        });
    engine.connect_source(Arc::clone(&buffer_mgr));
    engine.connect_storage(storage);
    engine.start();

    // Run the test for the specified duration
    let start_time = Instant::now();
    let mut cpu_usages = Vec::new();
    let mut memory_usages = Vec::new();

    // 在 main 或 run_test_scenario 函数中
    let pid = sysinfo::get_current_pid().expect("无法获取进程ID");

    while start_time.elapsed() < duration {
        tokio::time::sleep(Duration::from_millis(500)).await;

        // 1. 刷新 CPU（全局）
        sys.refresh_cpu();
        
        // 2. 刷新特定进程（内存）
        sys.refresh_process(pid);

        // 3. 采集 CPU
        cpu_usages.push(sys.global_cpu_info().cpu_usage());
        

        // 4. 采集进程内存 (重点修复)
        if let Some(process) = sys.process(pid) {
            // process.memory() 返回的是 Bytes (字节)
            // 转换为 MB: Bytes / 1024 / 1024
            let mem_mb = process.memory() as f64 / 1024.0 / 1024.0;
            memory_usages.push(mem_mb);
        }
    }

    // Collect stats
    let stats = buffer_mgr.get_stats();
    let avg_cpu_usage = cpu_usages.iter().sum::<f32>() / cpu_usages.len() as f32;
    let avg_memory_usage = memory_usages.iter().sum::<f64>() / memory_usages.len() as f64;

    // Shut down
    engine.shutdown();
    println!("Throughput: {:.1} writes/sec, {:.1} reads/sec", stats.write_rate, stats.pop_rate);
    println!("Buffer coverage/loss: {}", stats.overwrite_count);
    println!("Average CPU usage: {:.2}%", avg_cpu_usage);
    println!("Average Memory usage: {:.2} MB", avg_memory_usage);

    BenchmarkResult
    {
        sensor_count,
        duration_time: duration.as_secs(),
        throughput_write_rate: stats.write_rate,
        throughput_pop_rate: stats.pop_rate,
        buffer_utilization: stats.utilization,
        data_loss_count: stats.overwrite_count,
        avg_cpu_usage_percent: avg_cpu_usage,
        memory_usage_mb: avg_memory_usage,
    }
}
// Students are free to use any web framework they like for the dashboard.
// Fell free to change into your familar ones.

use hotaru::http::*;
use hotaru::prelude::*;
use std::collections::HashMap;

/// Gateway Axum API base (same process as `gateway` binary: `/sensor/:id`, etc.)
fn gateway_http_base() -> String {
    std::env::var("GATEWAY_HTTP").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
}

fn secs_to_hms_utc(secs_since_epoch: u64) -> String {
    let s = (secs_since_epoch % 86_400) as u32;
    let hh = s / 3600;
    let mm = (s % 3600) / 60;
    let ss = s % 60;
    format!("{:02}:{:02}:{:02}", hh, mm, ss)
}

/// Fetch last `n` aggregated points from gateway for a sensor.
/// Returns an array of points:
/// `{ t, avg, min, max, std_dev, anomaly_info }`
async fn fetch_sensor_points(
    gateway_base: &str,
    sensor_id: &str,
    n: usize,
) -> Vec<HashMap<String, hotaru::Value>> {
    if n == 0 {
        return Vec::new();
    }
    let url = format!(
        "{}/sensor/{}",
        gateway_base.trim_end_matches('/'),
        sensor_id
    );
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return (0..n).map(|_| empty_point()).collect();
        }
    };
    let Ok(resp) = client.get(&url).send().await else {
        return (0..n).map(|_| empty_point()).collect();
    };
    if !resp.status().is_success() {
        return (0..n).map(|_| empty_point()).collect();
    }
    let Ok(frames) = resp.json::<Vec<serde_json::Value>>().await else {
        return (0..n).map(|_| empty_point()).collect();
    };

    let mut points: Vec<HashMap<String, hotaru::Value>> = frames
        .into_iter()
        .filter_map(|f| {
            let sensor_info = f.get("sensor_info")?;
            let avg = sensor_info.get("avg_value")?.as_f64()?;
            let min = sensor_info.get("min_value")?.as_f64()?;
            let max = sensor_info.get("max_value")?.as_f64()?;
            let std_dev = sensor_info.get("std_dev")?.as_f64()?;

            let t = f
                .get("window_end")
                .and_then(|we| we.get("secs_since_epoch"))
                .and_then(|v| v.as_u64())
                .map(secs_to_hms_utc)
                .unwrap_or_else(|| "--:--:--".to_string());

            let anomaly_value = match f.get("anomaly_info") {
                Some(serde_json::Value::Object(map)) => {
                    let mut d = HashMap::new();
                    if let Some(v) = map.get("sensor_id").and_then(|v| v.as_str()) {
                        d.insert("sensor_id".to_string(), hotaru::Value::new(v));
                    }
                    if let Some(v) = map.get("anomaly_type").and_then(|v| v.as_str()) {
                        d.insert("anomaly_type".to_string(), hotaru::Value::new(v));
                    }
                    if let Some(v) = map.get("anomaly_value").and_then(|v| v.as_f64()) {
                        d.insert("anomaly_value".to_string(), hotaru::Value::new(v));
                    }
                    if let Some(v) = map.get("description").and_then(|v| v.as_str()) {
                        d.insert("description".to_string(), hotaru::Value::new(v));
                    }
                    hotaru::Value::new(d)
                }
                _ => hotaru::Value::None,
            };

            let mut p = HashMap::new();
            p.insert("t".to_string(), hotaru::Value::new(t));
            p.insert("avg".to_string(), hotaru::Value::new(avg));
            p.insert("min".to_string(), hotaru::Value::new(min));
            p.insert("max".to_string(), hotaru::Value::new(max));
            p.insert("std_dev".to_string(), hotaru::Value::new(std_dev));
            p.insert("anomaly_info".to_string(), anomaly_value);
            Some(p)
        })
        .collect();

    if points.len() > n {
        let start = points.len() - n;
        points = points.split_off(start);
    }
    while points.len() < n {
        let last = points
            .last()
            .cloned()
            .unwrap_or_else(|| empty_point());
        points.push(last);
    }

    points
}

fn empty_point() -> HashMap<String, hotaru::Value> {
    let mut p = HashMap::new();
    p.insert("t".to_string(), hotaru::Value::new("--:--:--"));
    p.insert("avg".to_string(), hotaru::Value::new(0.0));
    p.insert("min".to_string(), hotaru::Value::new(0.0));
    p.insert("max".to_string(), hotaru::Value::new(0.0));
    p.insert("std_dev".to_string(), hotaru::Value::new(0.0));
    p.insert("anomaly_info".to_string(), hotaru::Value::None);
    p
}

pub static APP: SApp = Lazy::new(|| {
    App::new()
        .binding("127.0.0.1:3000")
        .build()
});

endpoint! {
    APP.url("/"),
    pub index<HTTP> {
        akari_render!("home.html")
    }
}

// Proxies gateway JSON (`GET /sensor/:id`) into the shape expected by `home.html`.
endpoint!{
    APP.url("/data/<sensor_id>/<num_of_data>"),
    pub data<HTTP> {
        let sensor_id = req.param("sensor_id").unwrap();
        let num_of_data = req.param("num_of_data").unwrap().parse::<usize>().unwrap_or(0);
        let points = fetch_sensor_points(&gateway_http_base(), &sensor_id, num_of_data).await;
        return akari_json!({
            sensor_name: sensor_id,
            points: points,
        })
    }
}

endpoint!{
    APP.url("/registered_sensors"),
    pub dashboard<HTTP> {
        akari_json!({
            sensors: ["thermo-1", "thermo-2", "accel-1", "accel-2", "force-1"],
        }) 
    } 
 }

pub mod resource; 

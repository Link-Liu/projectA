// Students are free to use any web framework they like for the dashboard.
// Fell free to change into your familar ones.

use hotaru::http::*;
use hotaru::prelude::*;

/// Gateway Axum API base (same process as `gateway` binary: `/sensor/:id`, etc.)
fn gateway_http_base() -> String {
    std::env::var("GATEWAY_HTTP").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
}

/// Fetch aggregated `avg_value` series for charting: last `n` points from gateway.
async fn fetch_sensor_avg_series(gateway_base: &str, sensor_id: &str, n: usize) -> Vec<String> {
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
        Err(_) => return vec!["0".to_string(); n],
    };
    let Ok(resp) = client.get(&url).send().await else {
        return vec!["0".to_string(); n];
    };
    if !resp.status().is_success() {
        return vec!["0".to_string(); n];
    }
    let Ok(frames) = resp.json::<Vec<serde_json::Value>>().await else {
        return vec!["0".to_string(); n];
    };
    let mut values: Vec<f64> = frames
        .into_iter()
        .filter_map(|f| {
            f.get("sensor_info")?
                .get("avg_value")?
                .as_f64()
        })
        .collect();
    if values.len() > n {
        let start = values.len() - n;
        values = values.split_off(start);
    }
    while values.len() < n {
        values.push(values.last().copied().unwrap_or(0.0));
    }
    values.into_iter().map(|v| v.to_string()).collect()
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
        let data = fetch_sensor_avg_series(&gateway_http_base(), &sensor_id, num_of_data).await;
        return akari_json!({
            sensor_name: sensor_id,
            data: data,
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

// Students are free to use any web framework they like for the dashboard. 
// Fell free to change into your familar ones. 

use hotaru::prelude::*;
use hotaru::http::*; 

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

// Return the latest
endpoint!{
    APP.url("/data/<sensor_id>/<num_of_data>"),
    pub data<HTTP> {
        let sensor_id = req.param("sensor_id").unwrap();
        let num_of_data = req.param("num_of_data").unwrap().parse::<usize>().unwrap_or(0);
        let data = vec!["1"; num_of_data]; // TODO(student): read actual data from files written by gateway
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

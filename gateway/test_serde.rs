use serde::{Serialize, Deserialize};
use std::time::SystemTime;

#[derive(Serialize, Deserialize)]
struct Test {
    time: SystemTime,
}

fn main() {}

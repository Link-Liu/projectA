use dashboard::APP;

#[tokio::main]
async fn main() {
    // BONUS-PROCESS (runner): `dashboard` runs as an independent OS process.
    // It serves the UI on 127.0.0.1:3000 and fetches data from the gateway (8080) over HTTP.
    APP.clone().run().await;
}


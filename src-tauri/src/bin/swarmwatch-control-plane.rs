//! Headless control plane runner (dev/test).
//!
//! This starts the SwarmWatch control plane (HTTP + WS) on 127.0.0.1:4100
//! without launching the Tauri UI.

use swarmwatch_lib::control_plane;

#[tokio::main]
async fn main() {
    control_plane::spawn_control_plane().await;
    eprintln!("[swarmwatch-control-plane] listening on http://127.0.0.1:4100");

    // Keep process alive.
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}

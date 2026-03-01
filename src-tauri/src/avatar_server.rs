//! Legacy shim.
//!
//! Previously SwarmWatch spawned a Node-based server (`npm run server`).
//! We now embed the local control plane server in Rust (`control_plane.rs`).
//!
//! This module is kept to avoid breaking imports while the codebase migrates.

#[allow(dead_code)]
pub async fn spawn_avatar_server() {
    crate::control_plane::spawn_control_plane().await;
}

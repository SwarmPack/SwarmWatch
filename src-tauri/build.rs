use std::{
    env,
    fs,
    path::{Path, PathBuf},
};

/// Ensure the Tauri sidecar exists for the current Cargo target.
///
/// Tauri v2 expects sidecars referenced via `externalBin` to exist at build time,
/// otherwise even `cargo check` fails.
///
/// In CI we overwrite these with real binaries. Locally, we create a small
/// placeholder so `cargo check` works even if you haven't built the runner yet.
fn ensure_sidecar_placeholder() {
    let target = env::var("TARGET").unwrap_or_default();
    if target.is_empty() {
        return;
    }

    let is_windows = target.contains("windows");
    let file_name = if is_windows {
        format!("swarmwatch-runner-{target}.exe")
    } else {
        format!("swarmwatch-runner-{target}")
    };

    let path: PathBuf = Path::new("binaries").join(file_name);
    if path.exists() {
        return;
    }

    let _ = fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new("binaries")));

    // Keep placeholder minimal; it is overwritten in CI/release builds.
    if is_windows {
        let _ = fs::write(
            &path,
            b"swarmwatch-runner placeholder (build sidecar before release)\r\n",
        );
    } else {
        let _ = fs::write(
            &path,
            b"#!/bin/sh\n\necho 'swarmwatch-runner placeholder (build sidecar before running)' 1>&2\nexit 1\n",
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(&path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&path, perms);
            }
        }
    }
}

fn main() {
    ensure_sidecar_placeholder();
    tauri_build::build()
}

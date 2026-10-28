use rdm_core::{
    client::{read_frame, write_frame},
    launch,
};
use rdm_protocol::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::{Emitter, State};

struct Connection(Arc<AtomicBool>);
#[tauri::command]
async fn connect_service(
    app: tauri::AppHandle,
    state: State<'_, Connection>,
) -> Result<serde_json::Value, String> {
    launch::ensure_service().await.map_err(|e| e.to_string())?;
    if !state.0.swap(true, Ordering::SeqCst) {
        let flag = state.0.clone();
        tauri::async_runtime::spawn(async move {
            let result = async {
                let mut stream =
                    tokio::net::UnixStream::connect(rdm_core::paths::socket_path()).await?;
                write_frame(
                    &mut stream,
                    &Request {
                        version: VERSION,
                        id: "desktop-events".into(),
                        command: Command::Subscribe,
                    },
                )
                .await?;
                loop {
                    let response: Response = read_frame(&mut stream).await?;
                    app.emit("snapshot", response.data)?;
                }
                #[allow(unreachable_code)]
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
            .await;
            flag.store(false, Ordering::SeqCst);
            let _ = app.emit(
                "service-disconnected",
                result.err().map(|_| "Background service disconnected"),
            );
        });
    }
    rdm_core::client::request(Command::Snapshot)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn service_request(command: Command) -> Result<serde_json::Value, String> {
    rdm_core::client::request(command)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn register_browsers() -> Result<String, String> {
    let output = tokio::process::Command::new(launch::backend_path().map_err(|e| e.to_string())?)
        .arg("register-browsers")
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
#[tauri::command]
async fn open_download(id: String, folder: bool) -> Result<(), String> {
    let snapshot: Snapshot = serde_json::from_value(
        rdm_core::client::request(Command::Snapshot)
            .await
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let job = snapshot
        .downloads
        .iter()
        .find(|j| j.id == id)
        .ok_or("Download not found")?;
    let path = if folder {
        std::path::PathBuf::from(&job.destination)
    } else {
        if job.status != Status::Completed {
            return Err("Download is not complete".into());
        }
        std::path::Path::new(&job.destination).join(&job.filename)
    };
    tokio::process::Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
async fn start_at_login_status() -> bool {
    tokio::process::Command::new("systemctl")
        .args(["--user", "is-enabled", "rdm.service"])
        .output()
        .await
        .is_ok_and(|out| out.status.success())
}
#[tauri::command]
async fn set_start_at_login(enabled: bool) -> Result<(), String> {
    let status = tokio::process::Command::new("systemctl")
        .args([
            "--user",
            if enabled { "enable" } else { "disable" },
            "rdm.service",
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !status.status.success() {
        return Err("Install the .deb before enabling the login service.".into());
    }
    Ok(())
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Connection(Arc::new(AtomicBool::new(false))))
        .invoke_handler(tauri::generate_handler![
            connect_service,
            service_request,
            register_browsers,
            open_download,
            set_start_at_login,
            start_at_login_status
        ])
        .run(tauri::generate_context!())
        .expect("Could not launch Rust Download Manager");
}

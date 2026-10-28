use anyhow::{Result, bail};
use rdm_protocol::Command;
use std::{path::PathBuf, process::Stdio, time::Duration};
pub fn backend_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("RDM_BACKEND") {
        return Ok(path.into());
    }
    let current = std::env::current_exe()?;
    for path in [
        current.with_file_name("rdm-backend"),
        PathBuf::from("/usr/lib/rdm/rdm-backend"),
    ] {
        if path.is_file() {
            return Ok(path);
        }
    }
    bail!("Backend executable not found. Install the .deb or set RDM_BACKEND.")
}
pub async fn ensure_service() -> Result<()> {
    if crate::client::request(Command::Health).await.is_ok() {
        return Ok(());
    }
    if std::env::var_os("RDM_SOCKET").is_none() && std::env::var_os("RDM_DATA_DIR").is_none() {
        let _ = tokio::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        let _ = tokio::process::Command::new("systemctl")
            .args(["--user", "start", "rdm.service"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    if crate::client::request(Command::Health).await.is_err() {
        std::process::Command::new(backend_path()?)
            .arg("serve")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
    }
    for _ in 0..50 {
        if crate::client::request(Command::Health).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bail!("Background service did not start. Run rdm-backend serve to inspect the error.")
}

use anyhow::{Result, ensure};
use std::{
    env,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
};

pub fn home() -> PathBuf {
    PathBuf::from(env::var_os("HOME").expect("HOME is required"))
}
pub fn data_dir() -> PathBuf {
    env::var_os("RDM_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".local/share"))
                .join("rdm")
        })
}
pub fn socket_path() -> PathBuf {
    env::var_os("RDM_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| data_dir().join("run"))
                .join("rdm/service.sock")
        })
}
pub fn private_dir(path: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    let m = std::fs::symlink_metadata(path)?;
    ensure!(
        m.is_dir() && !m.file_type().is_symlink() && m.uid() == unsafe { libc::geteuid() },
        "Unsafe private directory"
    );
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn downloads_dir() -> String {
    if let Ok(out) = std::process::Command::new("xdg-user-dir")
        .arg("DOWNLOAD")
        .output()
    {
        let value = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if out.status.success() && value.starts_with('/') {
            return value;
        }
    }
    home().join("Downloads").to_string_lossy().into_owned()
}

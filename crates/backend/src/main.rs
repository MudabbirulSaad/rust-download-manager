use anyhow::{Context, Result, ensure};
use rdm_core::{
    client::{read_frame, write_frame},
    engine::Engine,
    paths,
};
use rdm_protocol::*;
use std::{
    os::{
        fd::AsRawFd,
        unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rdm=info".into()),
        )
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") => serve().await,
        Some("native-host") => native_host().await,
        Some("register-browsers") => register_browsers(),
        Some("status") => {
            println!(
                "{}",
                serde_json::to_string_pretty(&rdm_core::client::request(Command::Snapshot).await?)?
            );
            Ok(())
        }
        Some("request") => {
            let json = if let Some(arg) = args.get(1) {
                arg.clone()
            } else {
                let mut text = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)?;
                text
            };
            let command: Command = serde_json::from_str(&json)?;
            rdm_core::launch::ensure_service().await?;
            println!("{}", rdm_core::client::request(command).await?);
            Ok(())
        }
        _ => {
            println!(
                "Rust Download Manager backend 0.2.2\nUsage: rdm-backend serve | status | request '<command JSON>' | register-browsers | native-host"
            );
            Ok(())
        }
    }
}
async fn serve() -> Result<()> {
    let dir = paths::data_dir();
    paths::private_dir(&dir)?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join("service.lock"))?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "Service is already running"
    );
    let socket = paths::socket_path();
    paths::private_dir(socket.parent().context("Invalid socket path")?)?;
    if let Ok(meta) = std::fs::symlink_metadata(&socket) {
        ensure!(
            meta.file_type().is_socket() && meta.uid() == unsafe { libc::geteuid() },
            "Unsafe existing socket path"
        );
        std::fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    let engine = Engine::open(&dir)?;
    let scheduler = tokio::spawn(engine.clone().scheduler());
    tracing::info!("Download service ready");
    loop {
        tokio::select! {
            _=engine.stop.cancelled()=>break,
            _=tokio::signal::ctrl_c()=>{engine.stop.cancel();break;},
            _=termination()=>{engine.stop.cancel();break;},
            result=listener.accept()=>{
                let (stream,_)=result?;
                if stream.peer_cred()?.uid()!=unsafe{libc::geteuid()} { continue; }
                let engine=engine.clone();
                tokio::spawn(async move { if let Err(e)=connection(stream,engine).await {tracing::debug!(error=%e,"Client disconnected");} });
            }
        }
    }
    scheduler.await?;
    for _ in 0..200 {
        if engine.inner.lock().await.active.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    std::fs::remove_file(socket)?;
    Ok(())
}
async fn termination() {
    if let Ok(mut sig) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        sig.recv().await;
    }
}
async fn connection(mut stream: UnixStream, engine: Arc<Engine>) -> Result<()> {
    let request: Request =
        tokio::time::timeout(std::time::Duration::from_secs(10), read_frame(&mut stream)).await??;
    if request.version != VERSION {
        write_frame(
            &mut stream,
            &Response::failure(request.id, "Protocol version mismatch".into()),
        )
        .await?;
        return Ok(());
    }
    if matches!(request.command, Command::Subscribe) {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tokio::select! { _=engine.stop.cancelled()=>break, _=interval.tick()=>() }
            write_frame(
                &mut stream,
                &Response::success(
                    request.id.clone(),
                    serde_json::to_value(engine.snapshot().await)?,
                ),
            )
            .await?;
        }
    } else {
        let response = match engine.command(request.command).await {
            Ok(data) => Response::success(request.id, data),
            Err(e) => Response::failure(request.id, e.to_string()),
        };
        write_frame(&mut stream, &response).await?;
    }
    Ok(())
}
async fn native_host() -> Result<()> {
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    loop {
        let length = match input.read_u32_le().await {
            Ok(n) => n as usize,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        ensure!(
            length > 0 && length <= 1024 * 1024,
            "Invalid native message length"
        );
        let mut bytes = vec![0; length];
        input.read_exact(&mut bytes).await?;
        let result = async {
            let request: Request = serde_json::from_slice(&bytes)?;
            ensure!(request.version == VERSION, "Protocol version mismatch");
            ensure!(
                matches!(
                    request.command,
                    Command::Health | Command::Add { .. } | Command::AddMedia { .. }
                ),
                "Browser host only accepts health and download handoff messages"
            );
            rdm_core::launch::ensure_service().await?;
            Ok::<_, anyhow::Error>((
                request.id,
                rdm_core::client::request(request.command).await?,
            ))
        }
        .await;
        let response = match result {
            Ok((id, data)) => Response::success(id, data),
            Err(e) => Response::failure(String::new(), e.to_string()),
        };
        write_frame(&mut output, &response).await?;
        output.flush().await?;
    }
    Ok(())
}
fn register_browsers() -> Result<()> {
    let executable = std::env::current_exe()?.canonicalize()?;
    let dir = paths::data_dir();
    paths::private_dir(&dir)?;
    let wrapper = dir.join("native-host");
    let quoted = executable.to_string_lossy().replace('\'', "'\\''");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\nexec '{quoted}' native-host \"$@\"\n"),
    )?;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))?;
    let chromium_id = include_str!("../../../extensions/chromium-id.txt").trim();
    let mut chrome = serde_json::json!({"name":"org.rdm.download_manager","description":"Rust Download Manager","path":wrapper,"type":"stdio","allowed_origins":[format!("chrome-extension://{chromium_id}/")]});
    for relative in [
        ".config/BraveSoftware/Brave-Browser/NativeMessagingHosts",
        ".config/google-chrome/NativeMessagingHosts",
        ".config/chromium/NativeMessagingHosts",
    ] {
        let path = paths::home().join(relative);
        std::fs::create_dir_all(&path)?;
        std::fs::write(
            path.join("org.rdm.download_manager.json"),
            serde_json::to_vec_pretty(&chrome)?,
        )?;
    }
    chrome.as_object_mut().unwrap().remove("allowed_origins");
    chrome["allowed_extensions"] = serde_json::json!(["rdm@download-manager.local"]);
    let firefox = paths::home().join(".mozilla/native-messaging-hosts");
    std::fs::create_dir_all(&firefox)?;
    std::fs::write(
        firefox.join("org.rdm.download_manager.json"),
        serde_json::to_vec_pretty(&chrome)?,
    )?;
    println!(
        "Registered native host for Brave, Chrome, Chromium and Firefox. Load the matching extension, then use Test connection. Firefox Snap may prompt for native-host access."
    );
    Ok(())
}

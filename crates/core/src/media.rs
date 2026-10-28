//! Rust supervision of the pinned, open-source media provider.
use crate::{
    engine::{Engine, part_path, safe_filename, unique_filename},
    transfer,
};
use anyhow::{Context, Result, bail, ensure};
use rdm_protocol::{Download, Status};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use tokio_util::sync::CancellationToken;

fn source_root() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(root) = exe
            .ancestors()
            .find(|p| p.join("media/worker.py").is_file())
    {
        return root.into();
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn tools_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("RDM_MEDIA_DIR") {
        return path.into();
    }
    let installed = PathBuf::from("/usr/lib/rdm/media");
    if installed.join("worker.py").is_file() {
        installed
    } else {
        source_root().join(".cache/media")
    }
}
fn worker_path() -> PathBuf {
    let bundled = tools_dir().join("worker.py");
    if bundled.is_file() {
        bundled
    } else {
        source_root().join("media/worker.py")
    }
}
pub fn check_tools() -> Result<()> {
    for name in ["yt-dlp", "node"] {
        ensure!(
            tools_dir().join(name).is_file(),
            "Video tools are missing. Install the complete RDM package or run python3 scripts/fetch-media-tools.py in the source tree."
        );
    }
    ensure!(
        worker_path().is_file(),
        "The RDM media worker is missing. Reinstall RDM."
    );
    Ok(())
}
pub fn work_dir(job: &Download) -> PathBuf {
    Path::new(&job.destination).join(format!(".rdm-{}.media", job.id))
}
// Every helper and FFmpeg subprocess belongs to this process group. Drop covers
// early errors, cancellation and persistence failures, not only the happy path.
struct ProcessGroup(u32);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}
pub async fn run(engine: Arc<Engine>, id: &str, token: CancellationToken) -> Result<()> {
    let (job, context, settings) = {
        let inner = engine.inner.lock().await;
        (
            inner
                .snapshot
                .downloads
                .iter()
                .find(|j| j.id == id)
                .context("Missing video")?
                .clone(),
            inner.contexts.get(id).cloned(),
            inner.snapshot.settings.clone(),
        )
    };
    ensure!(!token.is_cancelled(), "Paused");
    check_tools()?;
    let part = part_path(&job);
    // A completed provider output is moved here atomically before hashing. This
    // is a durable marker distinct from yt-dlp's own resumable partial files.
    if job.sha256.is_some() || part.exists() {
        return finish(&engine, id, &part, &job, &token).await;
    }
    let directory = work_dir(&job);
    crate::paths::private_dir(&directory)?;
    let directory = directory.canonicalize()?;
    engine
        .update(id, true, |j| {
            j.media_stage = Some("extracting".into());
            j.resumable = true;
        })
        .await?;
    let global_share = if settings.speed_limit == 0 {
        0
    } else {
        (settings.speed_limit / settings.max_downloads as u32).max(1)
    };
    let rate_limit = match (global_share, job.speed_limit) {
        (0, limit) | (limit, 0) => limit,
        (a, b) => a.min(b),
    };
    let input = serde_json::json!({
        "url": job.url, "context": context, "directory": directory,
        "max_height": job.media.as_ref().context("Missing video options")?.max_height,
        "rate_limit": rate_limit,
        "yt_dlp": tools_dir().join("yt-dlp").canonicalize()?,
        "node": tools_dir().join("node").canonicalize()?,
    });
    let mut command = Command::new("python3");
    command
        .arg("-I")
        .arg(worker_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    let mut child = command
        .spawn()
        .context("Cannot start the video provider. Install python3 and ffmpeg.")?;
    let group = ProcessGroup(child.id().context("Missing video process")?);
    let mut stdin = child.stdin.take().context("Missing provider input")?;
    stdin.write_all(&serde_json::to_vec(&input)?).await?;
    drop(stdin);
    let mut lines = BufReader::new(child.stdout.take().context("Missing provider output")?).lines();
    let mut completed = None;
    let mut provider_error = None;
    let mut titled = false;
    loop {
        let line = tokio::select! {
            biased;
            _ = token.cancelled() => { drop(group); let _ = child.wait().await; bail!("Paused"); },
            line = lines.next_line() => line?,
        };
        let Some(line) = line else {
            break;
        };
        ensure!(line.len() < 16384, "Invalid video provider response");
        let event: serde_json::Value =
            serde_json::from_str(&line).context("Invalid video provider response")?;
        match event["event"].as_str() {
            Some("progress") => {
                if !titled && let Some(title) = event["title"].as_str() {
                    let mut inner = engine.inner.lock().await;
                    let others: Vec<_> = inner
                        .snapshot
                        .downloads
                        .iter()
                        .filter(|j| j.id != id)
                        .cloned()
                        .collect();
                    let filename = unique_filename(
                        &job.destination,
                        &format!("{}.mkv", safe_filename(title)),
                        &others,
                    );
                    if let Some(current) = inner.snapshot.downloads.iter_mut().find(|j| j.id == id)
                    {
                        current.filename = filename;
                    }
                    inner.store.save(&inner.snapshot)?;
                    titled = true;
                }
                engine
                    .update(id, false, |j| {
                        j.status = Status::Running;
                        j.media_stage = Some("downloading".into());
                        j.active_connections = 1;
                        j.downloaded = event["downloaded"].as_u64().unwrap_or(0) as usize;
                        j.total = event["total"].as_u64().map(|n| n as usize);
                        j.speed = event["speed"].as_u64().unwrap_or(0) as usize;
                    })
                    .await?
            }
            Some("stage") => {
                engine
                    .update(id, false, |j| {
                        j.status = Status::Running;
                        j.media_stage = Some("merging".into());
                        j.active_connections = 0;
                        j.speed = 0;
                    })
                    .await?
            }
            Some("error") => provider_error = event["message"].as_str().map(str::to_owned),
            Some("complete") => completed = Some(event),
            _ => (),
        }
    }
    let status = tokio::select! {
        _ = token.cancelled() => bail!("Paused"),
        status = child.wait() => status?,
    };
    drop(group);
    ensure!(
        status.success(),
        "{}",
        provider_error.unwrap_or_else(|| {
            "Video provider stopped. Check that python3 and ffmpeg are installed, then resume."
                .into()
        })
    );
    let event = completed.context("No downloadable video was found on this page")?;
    let name = event["path"].as_str().context("Missing completed video")?;
    ensure!(
        Path::new(name).components().count() == 1 && name.starts_with("media."),
        "Invalid media output path"
    );
    let output = directory.join(name);
    ensure!(
        output.symlink_metadata()?.file_type().is_file(),
        "Invalid media output file"
    );
    std::fs::File::open(&output)?.sync_all()?;
    let extension = output
        .extension()
        .and_then(|s| s.to_str())
        .context("Missing media extension")?;
    let title = safe_filename(event["title"].as_str().unwrap_or("Video"));
    let mut updated = job.clone();
    {
        let mut inner = engine.inner.lock().await;
        let others: Vec<_> = inner
            .snapshot
            .downloads
            .iter()
            .filter(|j| j.id != id)
            .cloned()
            .collect();
        updated.filename =
            unique_filename(&job.destination, &format!("{title}.{extension}"), &others);
        if let Some(current) = inner.snapshot.downloads.iter_mut().find(|j| j.id == id) {
            current.filename = updated.filename.clone();
        }
        // Name is durable before the marker is published, including crash recovery.
        inner.store.save(&inner.snapshot)?;
    }
    transfer::rename_noreplace(&output, &part)?;
    std::fs::File::open(&job.destination)?.sync_all()?;
    finish(&engine, id, &part, &updated, &token).await?;
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
async fn finish(
    engine: &Arc<Engine>,
    id: &str,
    part: &Path,
    job: &Download,
    token: &CancellationToken,
) -> Result<()> {
    let target = Path::new(&job.destination).join(&job.filename);
    let candidate = if part.exists() { part } else { &target };
    engine
        .update(id, true, |j| {
            j.media_stage = Some("verifying".into());
            j.speed = 0;
            j.active_connections = 0;
        })
        .await?;
    crate::content::verify_file(candidate, rdm_protocol::ExpectedContent::Media, None, token)
        .await?;
    let digest = transfer::hash_file(candidate, token).await?;
    if let Some(expected) = &job.sha256 {
        ensure!(
            *expected == digest,
            "Completed video bytes changed. Choose Restart."
        );
    }
    let size = candidate.metadata()?.len() as usize;
    engine
        .update(id, true, |j| {
            j.sha256 = Some(digest.clone());
            j.total = Some(size);
            j.downloaded = size;
            j.media_stage = Some("verifying".into());
        })
        .await?;
    transfer::finalize(engine, id, part, digest, token).await
}

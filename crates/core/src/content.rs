//! Content checks run before a whole-file digest can journal finalization.
use anyhow::{Context, Result, ensure};
use rdm_protocol::{Download, ExpectedContent};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;

pub fn infer_filename(filename: &str) -> ExpectedContent {
    let extension = Path::new(filename)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "html" | "htm" | "xhtml" => ExpectedContent::Html,
        "mp4" | "m4v" | "mov" | "mkv" | "webm" | "avi" | "flv" | "wmv" | "mpeg" | "mpg"
        | "m2ts" | "mp3" | "m4a" | "aac" | "ogg" | "opus" | "flac" | "wav" | "wma" | "aiff" => {
            ExpectedContent::Media
        }
        "zip" | "rar" | "7z" | "gz" | "bz2" | "xz" | "zst" | "tar" | "exe" | "msi" | "deb"
        | "rpm" | "dmg" | "iso" | "bin" | "pdf" | "docx" | "xlsx" | "pptx" | "odt" | "apk"
        | "png" | "jpg" | "jpeg" | "webp" | "gif" | "tif" | "tiff" | "ico" => {
            ExpectedContent::Binary
        }
        _ => ExpectedContent::Auto,
    }
}

pub fn expected(job: &Download) -> ExpectedContent {
    if job.media.is_some() {
        return ExpectedContent::Media;
    }
    job.expected_content
        .filter(|c| *c != ExpectedContent::Auto)
        .unwrap_or_else(|| {
            let source = url::Url::parse(&job.url)
                .ok()
                .map(|url| infer_filename(url.path()))
                .unwrap_or_default();
            if source != ExpectedContent::Auto {
                source
            } else {
                infer_filename(&job.filename)
            }
        })
}

pub fn observed(current: ExpectedContent, filename: &str, mime: Option<&str>) -> ExpectedContent {
    if matches!(current, ExpectedContent::Html | ExpectedContent::Media) {
        return current;
    }
    let by_name = infer_filename(filename);
    let mime = mime
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if by_name == ExpectedContent::Media || mime.starts_with("video/") || mime.starts_with("audio/")
    {
        ExpectedContent::Media
    } else if current == ExpectedContent::Binary {
        ExpectedContent::Binary
    } else if by_name != ExpectedContent::Auto {
        by_name
    } else if matches!(
        mime.as_str(),
        "application/octet-stream"
            | "application/pdf"
            | "application/zip"
            | "application/gzip"
            | "application/x-7z-compressed"
            | "application/x-rar-compressed"
    ) {
        ExpectedContent::Binary
    } else {
        ExpectedContent::Auto
    }
}

fn html_mime(mime: Option<&str>) -> bool {
    matches!(
        mime.unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "text/html" | "application/xhtml+xml"
    )
}

fn html_prefix(bytes: &[u8]) -> bool {
    let mut bytes = bytes;
    let utf16 = bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]);
    if utf16 {
        bytes = &bytes[2..];
    }
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        bytes = &bytes[3..];
    }
    let text: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| !utf16 || *b != 0)
        .map(|b| b.to_ascii_lowercase())
        .collect();
    let mut text = String::from_utf8_lossy(&text);
    loop {
        let trimmed = text.trim_start();
        if trimmed.starts_with("<!--")
            && let Some(end) = trimmed.find("-->")
        {
            text = std::borrow::Cow::Owned(trimmed[end + 3..].to_owned());
            continue;
        }
        if trimmed.starts_with("<?xml")
            && let Some(end) = trimmed.find("?>")
        {
            text = std::borrow::Cow::Owned(trimmed[end + 2..].to_owned());
            continue;
        }
        return [
            "<!doctype html",
            "<html",
            "<head",
            "<body",
            "<form",
            "<title",
            "<script",
        ]
        .iter()
        .any(|tag| {
            trimmed.strip_prefix(tag).is_some_and(|rest| {
                rest.as_bytes()
                    .first()
                    .is_none_or(|c| c.is_ascii_whitespace() || matches!(c, b'>' | b'/'))
            })
        });
    }
}

pub async fn verify_file(
    path: &Path,
    intent: ExpectedContent,
    mime: Option<&str>,
    token: &CancellationToken,
) -> Result<()> {
    ensure!(!token.is_cancelled(), "Paused");
    if matches!(intent, ExpectedContent::Auto | ExpectedContent::Html) {
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.file_type().is_file(),
        "The partial file was replaced. Choose Restart."
    );
    let file = tokio::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .await?;
    let mut prefix = Vec::new();
    file.take(8192).read_to_end(&mut prefix).await?;
    ensure!(
        !html_mime(mime) && !html_prefix(&prefix),
        "The server returned an HTML login or error page instead of the expected file. The partial file was kept. Sign in or refresh the source, then choose Restart."
    );
    if intent == ExpectedContent::Media {
        verify_media(path, metadata.len(), token).await?;
    }
    Ok(())
}

async fn bounded(reader: impl AsyncRead + Unpin, maximum: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(maximum + 1).read_to_end(&mut bytes).await?;
    ensure!(
        bytes.len() as u64 <= maximum,
        "Media verification output exceeded its safe limit. The partial file was kept."
    );
    Ok(bytes)
}

async fn verify_media(path: &Path, size: u64, token: &CancellationToken) -> Result<()> {
    // No playlists, network protocols or external data references are needed to
    // inspect a completed local container. Counting packets reads past headers.
    let parent = unsafe { libc::getpid() };
    let mut command = Command::new("ffprobe");
    command
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-format_whitelist",
            "mov,matroska,webm,avi,asf,flv,mpeg,mpegts,mp3,aac,flac,ogg,wav,aiff,amr,ac3,eac3",
            "-count_packets",
            "-show_entries",
            "format=format_name:stream=codec_type,codec_name,nb_read_packets",
            "-of",
            "json",
            "-i",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // SIGKILL cannot run Rust drop handlers. Arm parent-death termination before
    // exec and check the race where the backend died before prctl was installed.
    // The pre-exec hook calls only async-signal-safe OS functions.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
            }
            Ok(())
        });
    }
    let mut child = command.spawn().context(
        "Media verification needs FFprobe. Install ffmpeg, then resume; the partial file was kept.",
    )?;
    let stdout = child.stdout.take().context("Missing verifier output")?;
    let stderr = child
        .stderr
        .take()
        .context("Missing verifier diagnostics")?;
    let deadline = Duration::from_secs((30 + size / 1_048_576).clamp(30, 600));
    let output = tokio::select! {
        biased;
        _ = token.cancelled() => anyhow::bail!("Paused"),
        _ = tokio::time::sleep(deadline) => anyhow::bail!("Media verification timed out. The partial file was kept; resume to retry verification."),
        output = async { tokio::try_join!(bounded(stdout, 256 * 1024), bounded(stderr, 64 * 1024), async { Ok::<_, anyhow::Error>(child.wait().await?) }) } => output?,
    };
    ensure!(
        output.2.success() && output.1.is_empty(),
        "Media is invalid or truncated. The partial file was kept. Choose Restart or refresh the source."
    );
    let probe: serde_json::Value = serde_json::from_slice(&output.0)
        .context("Invalid FFprobe result; the partial file was kept")?;
    ensure!(
        readable_streams(&probe),
        "Media has missing or unreadable audio/video streams. The partial file was kept. Choose Restart or refresh the source."
    );
    Ok(())
}

fn readable_streams(probe: &serde_json::Value) -> bool {
    let Some(streams) = probe["streams"].as_array() else {
        return false;
    };
    let streams: Vec<_> = streams
        .iter()
        .filter(|stream| matches!(stream["codec_type"].as_str(), Some("audio" | "video")))
        .collect();
    !streams.is_empty()
        && streams.iter().all(|stream| {
            stream["codec_name"]
                .as_str()
                .is_some_and(|codec| codec != "unknown")
                && stream["nb_read_packets"]
                    .as_str()
                    .and_then(|n| n.parse::<u64>().ok())
                    .is_some_and(|n| n > 0)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_readable_audio_track_cannot_mask_an_unreadable_video_track() {
        let mut probe = serde_json::json!({"streams":[
            {"codec_type":"audio","codec_name":"aac","nb_read_packets":"20"},
            {"codec_type":"video","codec_name":"unknown","nb_read_packets":"0"}
        ]});
        assert!(!readable_streams(&probe));
        probe["streams"][1]["codec_name"] = "h264".into();
        probe["streams"][1]["nb_read_packets"] = "30".into();
        assert!(readable_streams(&probe));
    }
    #[test]
    fn recognizes_html_without_treating_an_embedded_tag_in_binary_as_a_page() {
        for sample in [
            b"<!-- header -->\n<!DOCTYPE HTML>".as_slice(),
            b"\xff\xfe<\0h\0t\0m\0l\0>",
            b"<?xml version='1.0'?><html>",
        ] {
            assert!(html_prefix(sample));
        }
        assert!(html_prefix(b"\xef\xbb\xbf\n<HTML>"));
        assert!(!html_prefix(b"PK\x03\x04binary data <html>"));
        assert!(!html_prefix(b"<HTMLCollection>document.forms"));
    }
    #[test]
    fn a_server_filename_cannot_weaken_an_explicit_expectation() {
        assert_eq!(
            observed(ExpectedContent::Binary, "login.html", Some("text/html")),
            ExpectedContent::Binary
        );
        assert_eq!(
            observed(ExpectedContent::Html, "file.zip", None),
            ExpectedContent::Html
        );
        assert_eq!(
            observed(ExpectedContent::Auto, "file.mp4", None),
            ExpectedContent::Media
        );
    }
    #[test]
    fn typescript_files_are_not_inferred_as_transport_stream_media() {
        assert_eq!(infer_filename("source.ts"), ExpectedContent::Auto);
        assert_eq!(
            observed(
                ExpectedContent::Auto,
                "source.ts",
                Some("application/typescript")
            ),
            ExpectedContent::Auto
        );
        assert_eq!(
            observed(ExpectedContent::Auto, "source.ts", Some("video/mp2t")),
            ExpectedContent::Media
        );
    }
}

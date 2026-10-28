use crate::engine::{Engine, part_path, safe_filename, unique_filename};
use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use rdm_protocol::*;
use reqwest::{Client, Response, StatusCode, header};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    fs::OpenOptions,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::OwnedSemaphorePermit,
};
use tokio_util::sync::CancellationToken;

const CHUNK: usize = 4 * 1024 * 1024;
struct Network {
    response: Response,
    _permit: OwnedSemaphorePermit,
}
fn client() -> Result<Client> {
    Ok(Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(30))
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()?)
}
async fn fetch(
    engine: &Arc<Engine>,
    client: &Client,
    initial: &str,
    context: &RequestContext,
    range: Option<(usize, usize)>,
    validator: Option<&str>,
    token: &CancellationToken,
) -> Result<Network> {
    let original = url::Url::parse(initial)?;
    let mut url = original.clone();
    for _ in 0..11 {
        ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none(),
            "Unsupported redirect"
        );
        let limit = engine
            .inner
            .lock()
            .await
            .snapshot
            .settings
            .connections_per_origin;
        let semaphore = engine
            .origins
            .lock()
            .await
            .entry(url.origin().ascii_serialization())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(limit)))
            .clone();
        let permit = tokio::select! { _=token.cancelled()=>bail!("Paused"), p=semaphore.acquire_owned()=>p? };
        let mut request = client
            .get(url.clone())
            .header(header::ACCEPT_ENCODING, "identity")
            .header(header::USER_AGENT, "RustDownloadManager/0.1");
        if url.origin() == original.origin() {
            if let Some(agent) = &context.user_agent {
                request = request.header(header::USER_AGENT, agent);
            }
            if let Some(referer) = &context.referer {
                request = request.header(header::REFERER, referer);
            }
            let cookies = context
                .cookies
                .iter()
                .filter(|c| cookie_matches(c, &url))
                .map(|c| format!("{}={}", c.name, c.value))
                .collect::<Vec<_>>()
                .join("; ");
            if !cookies.is_empty() {
                request = request.header(header::COOKIE, cookies);
            }
        }
        if let Some((start, end)) = range {
            request = request.header(header::RANGE, format!("bytes={start}-{end}"));
            if let Some(v) = validator {
                request = request.header(header::IF_RANGE, v);
            }
        }
        let response = tokio::select! { _=token.cancelled()=>bail!("Paused"), r=request.send()=>r.map_err(|e|e.without_url())? };
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(header::LOCATION)
                .context("Redirect has no location")?
                .to_str()?;
            url = url.join(location)?;
            drop(response);
            drop(permit);
            continue;
        }
        return Ok(Network {
            response,
            _permit: permit,
        });
    }
    bail!("Too many redirects")
}
fn cookie_matches(c: &ScopedCookie, url: &url::Url) -> bool {
    let host = url.host_str().unwrap_or_default();
    let domain = c.domain.trim_start_matches('.');
    let domain_match = host == domain || (!c.host_only && host.ends_with(&format!(".{domain}")));
    let cookie_path = if c.path.is_empty() { "/" } else { &c.path };
    let path_match = url.path() == cookie_path
        || (url.path().starts_with(cookie_path)
            && (cookie_path.ends_with('/') || url.path()[cookie_path.len()..].starts_with('/')));
    domain_match && path_match && (!c.secure || url.scheme() == "https")
}
fn content_range(response: &Response) -> Result<(usize, usize, usize)> {
    let value = response
        .headers()
        .get(header::CONTENT_RANGE)
        .context("Server omitted Content-Range")?
        .to_str()?;
    let (range, total) = value
        .strip_prefix("bytes ")
        .context("Invalid range unit")?
        .split_once('/')
        .context("Invalid range")?;
    let (start, end) = range.split_once('-').context("Invalid range")?;
    let (start, end, total) = (
        start.parse::<usize>()?,
        end.parse::<usize>()?,
        total.parse::<usize>()?,
    );
    ensure!(
        start <= end && end < total && total <= 9_007_199_254_740_991,
        "Invalid Content-Range bounds"
    );
    Ok((start, end, total))
}
fn validator(response: &Response) -> Option<String> {
    if let Some(tag) = response
        .headers()
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        && tag.starts_with('"')
        && tag.ends_with('"')
    {
        return Some(tag.to_owned());
    }
    let modified = response
        .headers()
        .get(header::LAST_MODIFIED)?
        .to_str()
        .ok()?;
    let date = response.headers().get(header::DATE)?.to_str().ok()?;
    let delta = chrono::DateTime::parse_from_rfc2822(date)
        .ok()?
        .signed_duration_since(chrono::DateTime::parse_from_rfc2822(modified).ok()?);
    (delta.num_seconds() >= 60).then(|| modified.to_owned())
}
fn status_check(response: &Response) -> Result<()> {
    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        bail!("Authorization expired or access denied. Send a fresh link from your browser.");
    }
    ensure!(
        response.status().is_success(),
        "Server returned HTTP {}",
        response.status().as_u16()
    );
    ensure!(
        response
            .headers()
            .get(header::CONTENT_ENCODING)
            .is_none_or(|v| v == "identity"),
        "Server returned encoded content that cannot be safely segmented"
    );
    Ok(())
}
async fn rate_wait(
    engine: &Arc<Engine>,
    id: &str,
    bytes: usize,
    token: &CancellationToken,
) -> Result<()> {
    let (global, local) = {
        let inner = engine.inner.lock().await;
        let settings = &inner.snapshot.settings;
        let media_count = inner
            .snapshot
            .downloads
            .iter()
            .filter(|j| j.media.is_some() && inner.active.contains_key(&j.id))
            .count() as u32;
        let reserved = if settings.speed_limit == 0 {
            0
        } else {
            (settings.speed_limit / settings.max_downloads as u32)
                .max(1)
                .saturating_mul(media_count)
        };
        (
            if settings.speed_limit == 0 {
                0
            } else {
                settings.speed_limit.saturating_sub(reserved).max(1)
            },
            inner
                .snapshot
                .downloads
                .iter()
                .find(|j| j.id == id)
                .map(|j| j.speed_limit)
                .unwrap_or(0),
        )
    };
    let mut deadline = Instant::now();
    if global > 0 {
        let mut next = engine.global_rate.lock().await;
        let slot = (*next).max(deadline);
        *next = slot + Duration::from_secs_f64(bytes as f64 / global as f64);
        deadline = *next;
    }
    if local > 0 {
        let mut rates = engine.job_rates.lock().await;
        let next = rates.entry(id.to_owned()).or_insert_with(Instant::now);
        let slot = (*next).max(Instant::now());
        *next = slot + Duration::from_secs_f64(bytes as f64 / local as f64);
        deadline = deadline.max(*next);
    }
    tokio::select! { _=token.cancelled()=>bail!("Paused"), _=tokio::time::sleep_until(deadline.into())=>Ok(()) }
}
async fn body_to_file(
    engine: &Arc<Engine>,
    id: &str,
    network: Network,
    start: usize,
    length: Option<usize>,
    token: &CancellationToken,
) -> Result<usize> {
    let path = {
        let inner = engine.inner.lock().await;
        part_path(
            inner
                .snapshot
                .downloads
                .iter()
                .find(|j| j.id == id)
                .context("Download removed")?,
        )
    };
    let mut file = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .await?;
    file.seek(std::io::SeekFrom::Start(start as u64)).await?;
    let mut stream = network.response.bytes_stream();
    let mut written = 0;
    let outcome = async {
        loop {
            let next =
                tokio::select! { _=token.cancelled()=>bail!("Paused"), next=stream.next()=>next };
            let Some(data) = next else {
                break;
            };
            let data = data.map_err(|e| e.without_url())?;
            ensure!(
                length.is_none_or(|n| written + data.len() <= n),
                "Server sent more bytes than requested"
            );
            rate_wait(engine, id, data.len(), token).await?;
            file.write_all(&data).await?;
            written += data.len();
            engine
                .update(id, false, |j| j.downloaded += data.len())
                .await?;
        }
        // Keep the origin permit until every byte has been read.
        drop(network._permit);
        ensure!(
            length.is_none_or(|n| written == n),
            "Connection ended before the requested range was complete"
        );
        file.sync_data().await?;
        Ok::<usize, anyhow::Error>(written)
    }
    .await;
    if outcome.is_err() {
        engine
            .update(id, false, |j| {
                j.downloaded = j.downloaded.saturating_sub(written)
            })
            .await?;
    }
    outcome
}
pub async fn run(engine: Arc<Engine>, id: &str, token: CancellationToken) -> Result<()> {
    let (mut job, context, settings) = {
        let inner = engine.inner.lock().await;
        (
            inner
                .snapshot
                .downloads
                .iter()
                .find(|j| j.id == id)
                .context("Missing download")?
                .clone(),
            inner.contexts.get(id).cloned().unwrap_or_default(),
            inner.snapshot.settings.clone(),
        )
    };
    if token.is_cancelled() {
        bail!("Paused");
    }
    let path = part_path(&job);
    // A persisted whole-file digest journals the final rename across crashes.
    if let Some(expected) = &job.sha256 {
        let target = Path::new(&job.destination).join(&job.filename);
        let candidate = if path.exists() { &path } else { &target };
        crate::content::verify_file(
            candidate,
            crate::content::expected(&job),
            job.response_content_type.as_deref(),
            &token,
        )
        .await?;
        ensure!(
            hash_file(candidate, &token).await? == *expected,
            "Completed file bytes changed before finalization. Choose Restart."
        );
        return finalize(&engine, id, &path, expected.clone(), &token).await;
    }
    // Fully synced bodies can await verification without a digest yet. Retrying
    // that local step needs neither range support nor any new response bytes.
    if job
        .total
        .is_some_and(|total| complete_ranges(&job.completed_ranges, total))
    {
        let metadata =
            std::fs::symlink_metadata(&path).context("Partial file is missing. Choose Restart.")?;
        ensure!(
            metadata.file_type().is_file() && Some(metadata.len() as usize) == job.total,
            "Partial file was replaced. Choose Restart."
        );
        return verify_and_finalize(&engine, id, &path, &job, &token).await;
    }
    let client = client()?;
    let probe = fetch_retry(
        &engine,
        &client,
        &job.url,
        &context,
        Some((0, 0)),
        None,
        &token,
    )
    .await?;
    let empty_file = probe.response.status() == StatusCode::RANGE_NOT_SATISFIABLE
        && probe
            .response
            .headers()
            .get(header::CONTENT_RANGE)
            .is_some_and(|v| v == "bytes */0");
    if !empty_file {
        status_check(&probe.response)?;
    }
    let range_total = if probe.response.status() == StatusCode::PARTIAL_CONTENT {
        let (start, end, total) = content_range(&probe.response)?;
        ensure!(
            start == 0 && end == 0,
            "Server returned an invalid probe range"
        );
        Some(total)
    } else {
        None
    };
    let total = if empty_file {
        Some(0)
    } else {
        range_total.or_else(|| {
            probe
                .response
                .content_length()
                .and_then(|n| usize::try_from(n).ok())
        })
    };
    let current_validator = validator(&probe.response);
    let resumable = range_total.is_some() && current_validator.is_some();
    let final_url = probe.response.url().to_string();
    let content_type = probe
        .response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        });
    if !job.completed_ranges.is_empty() {
        ensure!(
            path.is_file(),
            "Partial file is missing. Choose Restart to download again."
        );
        ensure!(
            resumable
                && job.validator == current_validator
                && job.total == total
                && job.final_url.as_deref() == Some(&final_url),
            "The server file changed or safe resume is unavailable. Choose Restart to avoid mixing file versions."
        );
        let size = std::fs::symlink_metadata(&path)?;
        ensure!(
            !size.file_type().is_symlink() && total.is_none_or(|n| size.len() == n as u64),
            "Partial file was replaced. Choose Restart."
        );
    }
    if job.completed_ranges.is_empty() {
        if std::fs::symlink_metadata(&path).is_ok() {
            std::fs::remove_file(&path)?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .await?;
        if let Some(n) = total {
            file.set_len(n as u64).await?;
        }
    }
    if job.downloaded == 0
        && let Some(name) = disposition_name(&probe.response)
    {
        let mut inner = engine.inner.lock().await;
        let others: Vec<Download> = inner
            .snapshot
            .downloads
            .iter()
            .filter(|j| j.id != id)
            .cloned()
            .collect();
        let filename = unique_filename(&job.destination, &name, &others);
        if let Some(j) = inner.snapshot.downloads.iter_mut().find(|j| j.id == id) {
            j.filename = filename.clone();
        }
        job.filename = filename;
    }
    let expected_content = crate::content::observed(
        crate::content::expected(&job),
        &job.filename,
        content_type.as_deref(),
    );
    job.expected_content = Some(expected_content);
    job.response_content_type = content_type.clone();
    engine
        .update(id, true, |j| {
            j.total = total;
            j.validator = current_validator.clone();
            j.resumable = resumable;
            j.final_url = Some(final_url);
            j.status = Status::Running;
            j.error = None;
            j.expected_content = Some(expected_content);
            j.response_content_type = content_type.clone();
            j.downloaded = j.completed_ranges.iter().map(|r| r.end - r.start).sum();
        })
        .await?;
    if empty_file {
        drop(probe);
        engine
            .update(id, true, |j| {
                j.completed_ranges = vec![ByteRange { start: 0, end: 0 }]
            })
            .await?;
    } else if resumable {
        drop(probe);
        let total = total.unwrap();
        let ranges: Vec<ByteRange> = (0..total)
            .step_by(CHUNK)
            .map(|start| ByteRange {
                start,
                end: (start + CHUNK).min(total),
            })
            .filter(|r| {
                !job.completed_ranges
                    .iter()
                    .any(|c| c.start == r.start && c.end == r.end)
            })
            .collect();
        let count = if total >= 16 * 1024 * 1024 {
            settings.connections_per_file
        } else {
            1
        };
        let child = token.child_token();
        let results = futures_util::stream::iter(ranges).map(|range| {
            let engine=engine.clone(); let client=client.clone(); let context=context.clone(); let child=child.clone(); let url=job.url.clone(); let validator=current_validator.clone();
            async move {
                engine.update(id,false,|j|j.active_connections+=1).await?;
                let outcome = async {
                    for attempt in 0..5 {
                        let network=fetch_retry(&engine,&client,&url,&context,Some((range.start,range.end-1)),validator.as_deref(),&child).await?;
                        status_check(&network.response)?;
                        ensure!(network.response.status()==StatusCode::PARTIAL_CONTENT,"Server ignored the requested range or file changed. Choose Restart.");
                        ensure!(content_range(&network.response)? == (range.start,range.end-1,total),"Server returned a mismatched range");
                        ensure!(crate::transfer::validator(&network.response).is_none_or(|v|Some(v)==validator),"Server file changed during transfer");
                        match body_to_file(&engine,id,network,range.start,Some(range.end-range.start),&child).await {
                            Ok(_)=>break,
                            Err(error) if attempt==4 || child.is_cancelled()=>return Err(error),
                            Err(_)=>tokio::select!{_=child.cancelled()=>bail!("Paused"),_=tokio::time::sleep(Duration::from_secs(1<<attempt))=>()},
                        }
                    }
                    engine.update(id,true,|j|j.completed_ranges.push(range)).await?;
                    Ok::<(),anyhow::Error>(())
                }.await;
                engine.update(id,false,|j|j.active_connections=j.active_connections.saturating_sub(1)).await?;
                if outcome.is_err() { child.cancel(); }
                outcome
            }
        }).buffer_unordered(count).collect::<Vec<_>>().await;
        for result in results {
            result?;
        }
    } else {
        let network = if range_total.is_none() {
            probe
        } else {
            drop(probe);
            fetch_retry(&engine, &client, &job.url, &context, None, None, &token).await?
        };
        status_check(&network.response)?;
        ensure!(
            network.response.status() == StatusCode::OK,
            "Server does not support a complete GET response"
        );
        engine
            .update(id, false, |j| j.active_connections = 1)
            .await?;
        let written = body_to_file(&engine, id, network, 0, total, &token).await?;
        engine
            .update(id, true, |j| {
                j.total = Some(written);
                j.completed_ranges = vec![ByteRange {
                    start: 0,
                    end: written,
                }];
                j.active_connections = 0;
            })
            .await?;
    }
    ensure!(!token.is_cancelled(), "Paused");
    verify_and_finalize(&engine, id, &path, &job, &token).await
}

fn complete_ranges(ranges: &[ByteRange], total: usize) -> bool {
    if total == 0 {
        return ranges.len() == 1 && ranges[0].start == 0 && ranges[0].end == 0;
    }
    let mut ranges: Vec<_> = ranges
        .iter()
        .map(|range| (range.start, range.end))
        .collect();
    ranges.sort_unstable();
    let mut position = 0;
    for (start, end) in ranges {
        if start != position || end <= start || end > total {
            return false;
        }
        position = end;
    }
    position == total
}

#[cfg(test)]
mod checkpoint_tests {
    use super::*;
    #[test]
    fn verification_requires_every_byte_to_be_checkpointed_even_if_the_file_is_preallocated() {
        assert!(!complete_ranges(&[], 10));
        assert!(!complete_ranges(&[ByteRange { start: 0, end: 5 }], 10));
        assert!(!complete_ranges(
            &[
                ByteRange { start: 0, end: 4 },
                ByteRange { start: 5, end: 10 }
            ],
            10
        ));
        assert!(!complete_ranges(
            &[
                ByteRange { start: 0, end: 6 },
                ByteRange { start: 5, end: 10 }
            ],
            10
        ));
        assert!(complete_ranges(
            &[
                ByteRange { start: 5, end: 10 },
                ByteRange { start: 0, end: 5 }
            ],
            10
        ));
        assert!(!complete_ranges(&[], 0));
        assert!(complete_ranges(&[ByteRange { start: 0, end: 0 }], 0));
    }
}

async fn verify_and_finalize(
    engine: &Arc<Engine>,
    id: &str,
    path: &Path,
    job: &Download,
    token: &CancellationToken,
) -> Result<()> {
    let intent = crate::content::expected(job);
    if intent == ExpectedContent::Media {
        engine
            .update(id, true, |j| {
                j.media_stage = Some("verifying".into());
                j.speed = 0;
                j.active_connections = 0;
            })
            .await?;
    }
    crate::content::verify_file(path, intent, job.response_content_type.as_deref(), token).await?;
    let digest = hash_file(path, token).await?;
    engine
        .update(id, true, |j| j.sha256 = Some(digest.clone()))
        .await?;
    finalize(engine, id, path, digest, token).await
}
pub(crate) async fn hash_file(path: &Path, token: &CancellationToken) -> Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .await?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        ensure!(!token.is_cancelled(), "Paused");
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub(crate) async fn finalize(
    engine: &Arc<Engine>,
    id: &str,
    path: &Path,
    digest: String,
    token: &CancellationToken,
) -> Result<()> {
    let mut inner = engine.inner.lock().await;
    ensure!(!token.is_cancelled(), "Paused");
    let current = inner
        .snapshot
        .downloads
        .iter()
        .find(|j| j.id == id)
        .context("Missing download")?
        .clone();
    let target = Path::new(&current.destination).join(&current.filename);
    if path.exists() {
        rename_noreplace(path,&target).context("Cannot finalize the file without overwriting an existing file. Move the conflicting file and resume.")?;
    }
    std::fs::File::open(&current.destination)?.sync_all()?;
    if let Some(job) = inner.snapshot.downloads.iter_mut().find(|j| j.id == id) {
        job.status = Status::Completed;
        job.sha256 = Some(digest);
        job.active_connections = 0;
        job.speed = 0;
        job.media_stage = None;
    }
    inner.contexts.remove(id);
    inner.store.save(&inner.snapshot)?;
    Ok(())
}
async fn fetch_retry(
    engine: &Arc<Engine>,
    client: &Client,
    url: &str,
    ctx: &RequestContext,
    range: Option<(usize, usize)>,
    validator: Option<&str>,
    token: &CancellationToken,
) -> Result<Network> {
    let mut last = None;
    for attempt in 0..5 {
        let result = fetch(engine, client, url, ctx, range, validator, token).await;
        let mut delay = 1u64 << attempt;
        match result {
            Ok(n)
                if n.response.status().is_server_error()
                    || n.response.status() == StatusCode::TOO_MANY_REQUESTS =>
            {
                delay = n
                    .response
                    .headers()
                    .get(header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(delay)
                    .min(300);
                last = Some(anyhow::anyhow!(
                    "Server returned HTTP {}",
                    n.response.status().as_u16()
                ));
            }
            Ok(n) => return Ok(n),
            Err(e) => last = Some(e),
        }
        if attempt < 4 {
            tokio::select! { _=token.cancelled()=>bail!("Paused"), _=tokio::time::sleep(Duration::from_secs(delay))=>() }
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("Network failure")))
}
fn disposition_name(response: &Response) -> Option<String> {
    let value = response
        .headers()
        .get(header::CONTENT_DISPOSITION)?
        .to_str()
        .ok()?;
    for part in value.split(';').skip(1) {
        let (key, value) = part.trim().split_once('=')?;
        if key.eq_ignore_ascii_case("filename") {
            return Some(safe_filename(value.trim_matches('"')));
        }
    }
    None
}
pub(crate) fn rename_noreplace(from: &Path, to: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let (from, to) = (
        std::ffi::CString::new(from.as_os_str().as_bytes())?,
        std::ffi::CString::new(to.as_os_str().as_bytes())?,
    );
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

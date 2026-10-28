use crate::store::Store;
use anyhow::{Result, bail, ensure};
use chrono::{Local, Timelike};
use rdm_protocol::*;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

pub struct Inner {
    pub snapshot: Snapshot,
    pub store: Store,
    pub active: HashMap<String, CancellationToken>,
    pub contexts: HashMap<String, RequestContext>,
}
pub struct Engine {
    pub inner: Mutex<Inner>,
    pub stop: CancellationToken,
    pub origins: Mutex<HashMap<String, Arc<Semaphore>>>,
    pub global_rate: Mutex<Instant>,
    pub job_rates: Mutex<HashMap<String, Instant>>,
}
impl Engine {
    pub fn open(dir: &Path) -> Result<Arc<Self>> {
        crate::paths::private_dir(dir)?;
        let store = Store::open(&dir.join("downloads.sqlite3"))?;
        let snapshot = store.load()?;
        store.save(&snapshot)?;
        Ok(Arc::new(Self {
            inner: Mutex::new(Inner {
                snapshot,
                store,
                active: HashMap::new(),
                contexts: HashMap::new(),
            }),
            stop: CancellationToken::new(),
            origins: Mutex::new(HashMap::new()),
            global_rate: Mutex::new(Instant::now()),
            job_rates: Mutex::new(HashMap::new()),
        }))
    }
    pub async fn snapshot(&self) -> Snapshot {
        self.inner.lock().await.snapshot.clone()
    }
    pub async fn update<F: FnOnce(&mut Download)>(
        &self,
        id: &str,
        persist: bool,
        f: F,
    ) -> Result<()> {
        let mut inner = self.inner.lock().await;
        if let Some(job) = inner.snapshot.downloads.iter_mut().find(|j| j.id == id) {
            f(job);
        }
        if persist {
            inner.store.save(&inner.snapshot)?;
        }
        Ok(())
    }
    pub async fn command(&self, command: Command) -> Result<serde_json::Value> {
        let command = match command {
            Command::AddMedia { spec } => {
                ensure!(spec.media.is_some(), "Missing video options");
                Command::Add { spec }
            }
            other => other,
        };
        match command {
            Command::Health => {
                return Ok(
                    serde_json::json!({"version": VERSION, "name": "rdm", "capabilities": ["media-v1", "content-validation-v1"]}),
                );
            }
            Command::Snapshot | Command::Subscribe => {
                return Ok(serde_json::to_value(self.snapshot().await)?);
            }
            Command::Shutdown => {
                self.stop.cancel();
                return Ok(serde_json::json!({"stopping": true}));
            }
            _ => (),
        }
        let mut inner = self.inner.lock().await;
        let result = match command {
            Command::Add { mut spec } => {
                if let Some(media) = &spec.media {
                    ensure!(
                        spec.expected_content != Some(ExpectedContent::Html),
                        "A media job cannot request HTML content"
                    );
                    ensure!(
                        [0, 360, 480, 720, 1080, 1440, 2160].contains(&media.max_height),
                        "Choose an available video quality"
                    );
                    crate::media::check_tools()?;
                }
                ensure!(
                    inner.snapshot.downloads.len() < 1000,
                    "History is full. Remove old entries first."
                );
                ensure!(spec.url.len() <= 8192, "URL is too long");
                let parsed = url::Url::parse(&spec.url)?;
                ensure!(
                    matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some(),
                    "Only HTTP and HTTPS URLs are supported"
                );
                ensure!(
                    parsed.username().is_empty() && parsed.password().is_none(),
                    "Credentials embedded in URLs are unsupported"
                );
                ensure!(
                    inner.snapshot.queues.iter().any(|q| q.id == spec.queue_id),
                    "Queue not found"
                );
                validate_context(&parsed, spec.context.as_ref())?;
                // Reauthorizing an interrupted signed-in download retains its durable ranges.
                if spec.context.is_some()
                    && let Some(existing) = inner.snapshot.downloads.iter_mut().find(|j| {
                        j.url == spec.url
                            && j.status == Status::NeedsAttention
                            && (j.authenticated || j.media.is_some())
                            && (!j.authenticated
                                || spec.context.as_ref().is_some_and(|c| !c.cookies.is_empty()))
                            && j.media.as_ref().map(|m| m.max_height)
                                == spec.media.as_ref().map(|m| m.max_height)
                    })
                {
                    let id = existing.id.clone();
                    existing.status = Status::Queued;
                    existing.error = None;
                    existing.authenticated = spec
                        .context
                        .as_ref()
                        .is_some_and(|ctx| !ctx.cookies.is_empty());
                    inner
                        .contexts
                        .insert(id.clone(), spec.context.take().unwrap());
                    inner.store.save(&inner.snapshot)?;
                    return Ok(serde_json::json!({"id": id}));
                }
                if let Some(key) = &spec.idempotency_key {
                    ensure!(key.len() <= 200, "Invalid handoff key");
                    if let Some(existing) = inner
                        .snapshot
                        .downloads
                        .iter()
                        .find(|j| j.idempotency_key.as_ref() == Some(key))
                    {
                        return Ok(serde_json::json!({"id": existing.id, "duplicate": true}));
                    }
                }
                let destination = PathBuf::from(
                    spec.destination
                        .unwrap_or_else(|| inner.snapshot.settings.download_directory.clone()),
                );
                ensure!(destination.is_absolute(), "Destination must be absolute");
                std::fs::create_dir_all(&destination)?;
                let destination = destination.canonicalize()?.to_string_lossy().into_owned();
                let guessed = spec.filename.unwrap_or_else(|| {
                    if spec.media.is_some() {
                        return "Video (resolving).mkv".into();
                    }
                    parsed
                        .path_segments()
                        .and_then(|mut p| p.next_back())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("download")
                        .to_owned()
                });
                let filename = unique_filename(
                    &destination,
                    &safe_filename(&guessed),
                    &inner.snapshot.downloads,
                );
                let expected_content = if spec.media.is_some() {
                    ExpectedContent::Media
                } else {
                    spec.expected_content
                        .filter(|c| *c != ExpectedContent::Auto)
                        .unwrap_or_else(|| crate::content::infer_filename(&guessed))
                };
                let id = uuid::Uuid::new_v4().to_string();
                let authenticated = spec.context.as_ref().is_some_and(|c| !c.cookies.is_empty());
                if let Some(ctx) = spec.context {
                    inner.contexts.insert(id.clone(), ctx);
                }
                inner.snapshot.downloads.insert(
                    0,
                    Download {
                        id: id.clone(),
                        url: spec.url,
                        filename,
                        destination,
                        queue_id: spec.queue_id,
                        status: Status::Queued,
                        total: None,
                        downloaded: 0,
                        speed: 0,
                        active_connections: 0,
                        completed_ranges: vec![],
                        validator: None,
                        resumable: false,
                        authenticated,
                        error: None,
                        created_at: chrono::Utc::now().to_rfc3339(),
                        speed_limit: 0,
                        idempotency_key: spec.idempotency_key,
                        final_url: None,
                        sha256: None,
                        media: spec.media,
                        media_stage: None,
                        expected_content: Some(expected_content),
                        response_content_type: None,
                    },
                );
                serde_json::json!({"id": id})
            }
            Command::Control { ids, action } => {
                for id in ids {
                    ensure!(
                        !inner.active.contains_key(&id)
                            || !matches!(action, Action::Resume | Action::Restart | Action::Remove),
                        "Pause the download and wait for its connections to stop first"
                    );
                    if let Some(token) = inner.active.get(&id) {
                        token.cancel();
                    }
                    if matches!(action, Action::Remove) {
                        inner.snapshot.downloads.retain(|j| j.id != id);
                        inner.contexts.remove(&id);
                        continue;
                    }
                    let has_context = inner.contexts.contains_key(&id);
                    let job = inner
                        .snapshot
                        .downloads
                        .iter_mut()
                        .find(|j| j.id == id)
                        .ok_or_else(|| anyhow::anyhow!("Download not found"))?;
                    if job.status == Status::Completed {
                        continue;
                    }
                    match action {
                        Action::Pause => job.status = Status::Paused,
                        Action::Cancel => job.status = Status::Cancelled,
                        Action::Resume | Action::Restart => {
                            ensure!(
                                !job.authenticated || has_context,
                                "Send this download from the browser again to refresh its authorization"
                            );
                            if matches!(action, Action::Restart) {
                                job.completed_ranges.clear();
                                job.downloaded = 0;
                                job.validator = None;
                                job.sha256 = None;
                                job.response_content_type = None;
                                let part = part_path(job);
                                if part.exists() {
                                    std::fs::remove_file(part)?;
                                }
                                if job.media.is_some() {
                                    let dir = crate::media::work_dir(job);
                                    if dir.exists() {
                                        std::fs::remove_dir_all(dir)?;
                                    }
                                    job.media_stage = None;
                                }
                            }
                            job.status = Status::Queued;
                            job.error = None;
                        }
                        _ => (),
                    }
                }
                serde_json::json!({})
            }
            Command::PauseAll => {
                for token in inner.active.values() {
                    token.cancel();
                }
                for job in &mut inner.snapshot.downloads {
                    if matches!(
                        job.status,
                        Status::Running | Status::Resolving | Status::Queued
                    ) {
                        job.status = Status::Paused;
                    }
                }
                serde_json::json!({})
            }
            Command::SetDownloadLimit {
                id,
                bytes_per_second,
            } => {
                ensure!(
                    !inner.active.contains_key(&id)
                        || !inner
                            .snapshot
                            .downloads
                            .iter()
                            .any(|j| j.id == id && j.media.is_some()),
                    "Pause this video before changing its speed limit, then resume."
                );
                let job = inner
                    .snapshot
                    .downloads
                    .iter_mut()
                    .find(|j| j.id == id)
                    .ok_or_else(|| anyhow::anyhow!("Download not found"))?;
                job.speed_limit = bytes_per_second;
                serde_json::json!({})
            }
            Command::UpsertQueue { queue } => {
                ensure!(
                    !queue.id.is_empty()
                        && queue.id.len() <= 100
                        && !queue.name.trim().is_empty()
                        && queue.name.len() <= 100,
                    "Invalid queue"
                );
                ensure!(
                    (1..=20).contains(&queue.concurrency),
                    "Queue concurrency must be 1–20"
                );
                ensure!(
                    queue.start_time.is_some() == queue.stop_time.is_some(),
                    "Set both start and stop times"
                );
                for t in [&queue.start_time, &queue.stop_time].into_iter().flatten() {
                    ensure!(parse_time(t).is_some(), "Time must use HH:MM");
                }
                if let Some(q) = inner.snapshot.queues.iter_mut().find(|q| q.id == queue.id) {
                    *q = queue;
                } else {
                    ensure!(inner.snapshot.queues.len() < 50, "Too many queues");
                    inner.snapshot.queues.push(queue);
                }
                serde_json::json!({})
            }
            Command::SetSettings { settings } => {
                if settings.speed_limit != inner.snapshot.settings.speed_limit
                    || settings.max_downloads != inner.snapshot.settings.max_downloads
                {
                    ensure!(
                        !inner
                            .snapshot
                            .downloads
                            .iter()
                            .any(|j| j.media.is_some() && inner.active.contains_key(&j.id)),
                        "Pause active videos before changing global speed or concurrency settings."
                    );
                }
                ensure!(
                    (1..=20).contains(&settings.max_downloads)
                        && (1..=16).contains(&settings.connections_per_file)
                        && (1..=16).contains(&settings.connections_per_origin),
                    "Invalid connection limits"
                );
                ensure!(
                    Path::new(&settings.download_directory).is_absolute(),
                    "Download directory must be absolute"
                );
                // A live semaphore cannot be replaced without violating the per-origin ceiling.
                if settings.connections_per_origin != inner.snapshot.settings.connections_per_origin
                {
                    ensure!(
                        inner.active.is_empty(),
                        "Pause active downloads before changing the per-origin limit"
                    );
                    self.origins.lock().await.clear();
                }
                inner.snapshot.settings = settings;
                serde_json::json!({})
            }
            _ => bail!("Unsupported command"),
        };
        if let Err(error) = inner.store.save(&inner.snapshot) {
            self.stop.cancel();
            return Err(error);
        }
        Ok(result)
    }
    pub async fn scheduler(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(Duration::from_millis(250));
        let mut previous = HashMap::<String, (Instant, usize)>::new();
        loop {
            tokio::select! { _ = self.stop.cancelled() => break, _ = ticker.tick() => () }
            let mut inner = self.inner.lock().await;
            let now = Instant::now();
            for job in &mut inner.snapshot.downloads {
                let old = previous
                    .entry(job.id.clone())
                    .or_insert((now, job.downloaded));
                let elapsed = now.duration_since(old.0).as_secs_f64();
                if elapsed >= 0.75 {
                    if job.media.is_none() {
                        job.speed = if job.status == Status::Running {
                            (job.downloaded.saturating_sub(old.1) as f64 / elapsed) as usize
                        } else {
                            0
                        };
                    }
                    *old = (now, job.downloaded);
                }
            }
            let queues = inner.snapshot.queues.clone();
            for queue in queues {
                let allowed = queue_open(&queue);
                let active_ids: Vec<String> = inner
                    .snapshot
                    .downloads
                    .iter()
                    .filter(|j| j.queue_id == queue.id && inner.active.contains_key(&j.id))
                    .map(|j| j.id.clone())
                    .collect();
                if !allowed {
                    for id in active_ids {
                        if let Some(t) = inner.active.get(&id) {
                            t.cancel();
                        }
                        if let Some(job) = inner.snapshot.downloads.iter_mut().find(|j| {
                            j.id == id && matches!(j.status, Status::Running | Status::Resolving)
                        }) {
                            job.status = Status::Queued;
                        }
                    }
                    continue;
                }
                let queue_active = active_ids.len();
                let ids: Vec<String> = inner
                    .snapshot
                    .downloads
                    .iter()
                    .rev()
                    .filter(|j| {
                        j.queue_id == queue.id
                            && j.status == Status::Queued
                            && !inner.active.contains_key(&j.id)
                    })
                    .map(|j| j.id.clone())
                    .collect();
                for (offset, id) in ids.into_iter().enumerate() {
                    if inner.active.len() >= inner.snapshot.settings.max_downloads
                        || queue_active + offset >= queue.concurrency
                    {
                        break;
                    }
                    let token = self.stop.child_token();
                    inner.active.insert(id.clone(), token.clone());
                    if let Some(job) = inner.snapshot.downloads.iter_mut().find(|j| j.id == id) {
                        job.status = Status::Resolving;
                    }
                    let engine = self.clone();
                    tokio::spawn(async move {
                        let is_media = engine
                            .snapshot()
                            .await
                            .downloads
                            .iter()
                            .any(|j| j.id == id && j.media.is_some());
                        let outcome = if is_media {
                            crate::media::run(engine.clone(), &id, token.clone()).await
                        } else {
                            crate::transfer::run(engine.clone(), &id, token.clone()).await
                        };
                        let mut inner = engine.inner.lock().await;
                        inner.active.remove(&id);
                        if let Some(job) = inner.snapshot.downloads.iter_mut().find(|j| j.id == id)
                        {
                            job.active_connections = 0;
                            job.speed = 0;
                            if let Err(error) = outcome {
                                if job.media.is_none() {
                                    job.downloaded =
                                        job.completed_ranges.iter().map(|r| r.end - r.start).sum();
                                }
                                if !token.is_cancelled() {
                                    job.status = Status::NeedsAttention;
                                    job.error = Some(error.to_string());
                                }
                            }
                        }
                        if let Err(e) = inner.store.save(&inner.snapshot) {
                            tracing::error!(error=%e, "Cannot persist download state");
                        }
                    });
                }
            }
        }
        let inner = self.inner.lock().await;
        for token in inner.active.values() {
            token.cancel();
        }
    }
}

pub fn part_path(job: &Download) -> PathBuf {
    Path::new(&job.destination).join(format!(".rdm-{}.part", job.id))
}
pub fn safe_filename(value: &str) -> String {
    let s: String = value
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\'))
        .take(180)
        .collect();
    let s = s.trim().trim_matches('.');
    if s.is_empty() {
        "download".into()
    } else {
        s.to_owned()
    }
}
pub fn unique_filename(dir: &str, name: &str, jobs: &[Download]) -> String {
    let path = Path::new(name);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = path
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    for index in 0..10000 {
        let candidate = if index == 0 {
            name.to_owned()
        } else {
            format!("{stem} ({index}){ext}")
        };
        if !Path::new(dir).join(&candidate).exists()
            && !jobs.iter().any(|j| {
                j.destination == dir && j.filename == candidate && j.status != Status::Cancelled
            })
        {
            return candidate;
        }
    }
    format!("{}-{name}", uuid::Uuid::new_v4())
}
fn validate_context(url: &url::Url, context: Option<&RequestContext>) -> Result<()> {
    if let Some(ctx) = context {
        ensure!(ctx.cookies.len() <= 200, "Too many cookies");
        let host = url.host_str().unwrap_or_default();
        for c in &ctx.cookies {
            let domain = c.domain.trim_start_matches('.');
            ensure!(
                !domain.is_empty()
                    && (host == domain || (!c.host_only && host.ends_with(&format!(".{domain}")))),
                "Cookie is outside the download origin"
            );
            ensure!(
                c.name.len() + c.value.len() < 16384
                    && !c.name.contains(['\r', '\n', ';', '='])
                    && !c.value.contains(['\r', '\n', ';']),
                "Invalid cookie"
            );
        }
    }
    Ok(())
}
pub fn parse_time(value: &str) -> Option<u32> {
    let (h, m) = value.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}
pub fn queue_open(q: &Queue) -> bool {
    let now = Local::now();
    queue_open_at(q, now.hour() * 60 + now.minute())
}
pub fn queue_open_at(q: &Queue, minute: u32) -> bool {
    if !q.enabled {
        return false;
    }
    match (
        q.start_time.as_deref().and_then(parse_time),
        q.stop_time.as_deref().and_then(parse_time),
    ) {
        (Some(start), Some(stop)) if start < stop => minute >= start && minute < stop,
        (Some(start), Some(stop)) if start > stop => minute >= start || minute < stop,
        _ => true,
    }
}

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const VERSION: u32 = 1;
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Resolving,
    Running,
    Paused,
    NeedsAttention,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct RequestContext {
    pub cookies: Vec<ScopedCookie>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ScopedCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub host_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct MediaOptions {
    /// Zero selects the best available quality; other values cap video height.
    pub max_height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AddSpec {
    pub url: String,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default = "main_queue")]
    pub queue_id: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub context: Option<RequestContext>,
    #[serde(default)]
    pub media: Option<MediaOptions>,
}
fn main_queue() -> String {
    "main".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Download {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub destination: String,
    pub queue_id: String,
    pub status: Status,
    pub total: Option<usize>,
    pub downloaded: usize,
    pub speed: usize,
    pub active_connections: usize,
    pub completed_ranges: Vec<ByteRange>,
    pub validator: Option<String>,
    pub resumable: bool,
    pub authenticated: bool,
    pub error: Option<String>,
    pub created_at: String,
    pub speed_limit: u32,
    pub idempotency_key: Option<String>,
    pub final_url: Option<String>,
    pub sha256: Option<String>,
    #[serde(default)]
    pub media: Option<MediaOptions>,
    #[serde(default)]
    pub media_stage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Queue {
    pub id: String,
    pub name: String,
    pub concurrency: usize,
    pub enabled: bool,
    pub start_time: Option<String>,
    pub stop_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Settings {
    pub max_downloads: usize,
    pub connections_per_file: usize,
    pub connections_per_origin: usize,
    pub speed_limit: u32,
    pub download_directory: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            max_downloads: 3,
            connections_per_file: 4,
            connections_per_origin: 8,
            speed_limit: 0,
            download_directory: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Snapshot {
    pub downloads: Vec<Download>,
    pub queues: Vec<Queue>,
    pub settings: Settings,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Pause,
    Resume,
    Restart,
    Cancel,
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Health,
    Snapshot,
    Subscribe,
    Add { spec: AddSpec },
    AddMedia { spec: AddSpec },
    Control { ids: Vec<String>, action: Action },
    PauseAll,
    SetDownloadLimit { id: String, bytes_per_second: u32 },
    UpsertQueue { queue: Queue },
    SetSettings { settings: Settings },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    pub id: String,
    pub command: Command,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub version: u32,
    pub id: String,
    pub ok: bool,
    pub data: serde_json::Value,
    pub error: Option<String>,
}
impl Response {
    pub fn success(id: String, data: serde_json::Value) -> Self {
        Self {
            version: VERSION,
            id,
            ok: true,
            data,
            error: None,
        }
    }
    pub fn failure(id: String, error: String) -> Self {
        Self {
            version: VERSION,
            id,
            ok: false,
            data: serde_json::Value::Null,
            error: Some(error),
        }
    }
}

pub fn typescript() -> String {
    let declarations = [
        Status::decl(),
        ByteRange::decl(),
        ScopedCookie::decl(),
        RequestContext::decl(),
        MediaOptions::decl(),
        AddSpec::decl(),
        Download::decl(),
        Queue::decl(),
        Settings::decl(),
        Snapshot::decl(),
        Action::decl(),
        Command::decl(),
    ];
    format!(
        "// Generated by cargo run -p rdm-protocol --example export. Do not edit.\n{}\n",
        declarations
            .iter()
            .map(|d| format!("export {d}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

use rdm_core::{
    engine::{Engine, queue_open_at, safe_filename},
    store::Store,
};
use rdm_protocol::*;
#[tokio::test]
async fn content_intent_survives_history_reload_and_legacy_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path()).unwrap();
    engine.command(serde_json::from_value(serde_json::json!({"type":"add", "spec":{
        "url":"https://example.com/file.zip", "filename":"saved.html", "expected_content":"html",
        "destination":directory.path().to_str().unwrap()
    }})).unwrap()).await.unwrap();
    let mut snapshot = engine.snapshot().await;
    assert_eq!(
        snapshot.downloads[0].expected_content,
        Some(ExpectedContent::Html)
    );
    drop(engine);
    let store = Store::open(&directory.path().join("downloads.sqlite3")).unwrap();
    assert_eq!(
        store.load().unwrap().downloads[0].expected_content,
        Some(ExpectedContent::Html)
    );
    snapshot.downloads[0].expected_content = None;
    store.save(&snapshot).unwrap();
    let recovered = store.load().unwrap();
    assert_eq!(
        recovered.downloads[0].expected_content,
        Some(ExpectedContent::Binary)
    );
    assert_eq!(recovered.downloads[0].id, snapshot.downloads[0].id);
}
#[test]
fn overnight_schedules_and_disabled_queues() {
    let mut q = Queue {
        id: "night".into(),
        name: "Night".into(),
        concurrency: 1,
        enabled: true,
        start_time: Some("22:00".into()),
        stop_time: Some("06:00".into()),
    };
    assert!(queue_open_at(&q, 23 * 60));
    assert!(queue_open_at(&q, 2 * 60));
    assert!(!queue_open_at(&q, 12 * 60));
    q.enabled = false;
    assert!(!queue_open_at(&q, 23 * 60));
}
#[test]
fn filenames_cannot_escape_the_download_directory() {
    for name in ["../../file.zip", "..\\..\\file.zip", "\0\r\n", "..."] {
        let safe = safe_filename(name);
        assert!(
            !safe.contains('/')
                && !safe.contains('\\')
                && !safe.is_empty()
                && !safe.chars().any(char::is_control)
        );
    }
}
#[tokio::test]
async fn rejects_cookie_from_a_different_origin_and_invalid_queue() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path()).unwrap();
    let spec = AddSpec {
        url: "https://example.com/file".into(),
        destination: Some(dir.path().to_str().unwrap().into()),
        filename: None,
        media: None,
        expected_content: None,
        queue_id: "main".into(),
        idempotency_key: None,
        context: Some(RequestContext {
            cookies: vec![ScopedCookie {
                name: "session".into(),
                value: "secret".into(),
                domain: "unrelated.test".into(),
                path: "/".into(),
                secure: true,
                host_only: true,
            }],
            ..Default::default()
        }),
    };
    assert!(
        engine
            .command(Command::Add { spec })
            .await
            .unwrap_err()
            .to_string()
            .contains("origin")
    );
    assert!(engine.snapshot().await.downloads.is_empty());
}
#[test]
fn settings_survive_sqlite_reload() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("state.db")).unwrap();
    let mut snapshot = store.load().unwrap();
    snapshot.settings.max_downloads = 7;
    store.save(&snapshot).unwrap();
    assert_eq!(store.load().unwrap().settings.max_downloads, 7);
}

#[tokio::test]
async fn public_browser_context_does_not_clear_required_cookie_authorization() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path()).unwrap();
    let mut spec: AddSpec = serde_json::from_value(serde_json::json!({
        "url":"https://example.com/file", "destination":dir.path(),
        "idempotency_key":"same-handoff", "context":{"cookies":[{
            "name":"session","value":"approved","domain":"example.com",
            "path":"/","secure":true,"host_only":true
        }]}
    }))
    .unwrap();
    engine
        .command(Command::Add { spec: spec.clone() })
        .await
        .unwrap();
    let id = engine.snapshot().await.downloads[0].id.clone();
    engine
        .update(&id, true, |job| job.status = Status::NeedsAttention)
        .await
        .unwrap();
    engine.inner.lock().await.contexts.clear();
    spec.context = Some(RequestContext::default());
    engine.command(Command::Add { spec }).await.unwrap();
    let state = engine.snapshot().await;
    assert!(state.downloads[0].authenticated);
    assert_eq!(state.downloads[0].status, Status::NeedsAttention);
    assert!(!engine.inner.lock().await.contexts.contains_key(&id));
}

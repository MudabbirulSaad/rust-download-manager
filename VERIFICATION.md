# Verification — RDM 0.2.2 — 2026-10-28

## Tested environments

The native binaries were built on Ubuntu 24.04 amd64 with Rust 1.99.1 and Node 24.21.0. Fresh installation was established on Ubuntu **24.04** and **26.04** runtime images; the 0.2.2 package is additionally checked as an upgrade in both environments, with unchanged package dependencies. Both runtime images verify that Rust/Cargo, a global Node executable and GCC are absent. RDM bundles a private Node runtime for its media provider and depends on Ubuntu Python/FFmpeg packages. Xvfb and WebKit WebDriver are test harness dependencies.

| Check | Result |
| --- | --- |
| Rust unit/integration tests | 5 passed |
| Local video/provider acceptance groups | 9 groups: recovery/integrity, Referer context, plain-text HLS with image-named fragments and required Origin, actionable unsupported-player errors |
| Real YouTube extraction | Passed with both audio and video tracks verified |
| React, video source selection and extension boundary tests | 29 passed |
| Transfer/backend acceptance checks on Ubuntu 24.04 | 22 passed |
| Transfer/backend acceptance checks on Ubuntu 26.04 | 22 passed |
| Permission-denied and 1 MiB full-disk fixtures | Both passed on both releases |
| Real Tauri/WebKitGTK desktop tests | Connected, downloaded, continued with all GUI windows closed, reconnected and displayed persisted details on both releases |
| Brave unpacked extension | Native connection, authenticated handoff, on-video overlay, repeated-click deduplication, overlay toggle, plain-text HLS detection, popup capture from embedded frames, and YouTube SPA/quality dispatch passed |
| Firefox Snap 151.0.3 temporary extension | Native connection, authenticated handoff and on-video overlay produced exact expected bytes through the actual portal/proxy path |
| Rust Clippy | Passed with warnings denied |
| Frontend production build and extension builds | Passed |
| Production npm dependency audit | No vulnerabilities reported |

## Video checks

Actual Brave content scripts observe manifests through Resource Timing and enqueue them through native messaging. Tests cover a blob player fetching a DASH manifest, and a player serving HLS as `playlist.txt`/`index.txt` with `text/plain` MIME types and TS fragments named `.png`. The extension probes only observed playlist-like text URLs after a user action, checks for HLS headers, prefers the master playlist, and rejects ordinary text/HTML and individual segments. The popup also resolves an embedded frame instead of downloading its outer page. FFprobe confirms playable audio and video in the output.

A protected local CDN requires public Origin context for every disguised-HLS request. The real worker passes this test without cookies or page-query tokens. A real browser diagnostic against the reported player also resolves its actual HLS master with the updated content script; the pinned provider enumerates its 720p stream when supplied with public Origin/Referer context. This establishes detection and access, not completion or integrity of the entire external video.

A separate real Firefox Snap test confirms Resource Timing manifest detection without changing user profiles or native-host registration. Unit checks cover disguised-playlist validation, CORS failure, direct-video/YouTube dispatch, master selection, segment rejection, URL-token handling and the visible error-to-details action.

`tests/media_acceptance.py` generates playable MP4 and separate DASH audio/video fixtures with FFmpeg, then downloads them through the actual Rust service and pinned yt-dlp worker. It checks exact byte hashes, real FFmpeg merging with both tracks confirmed by FFprobe, pause/resume, forced service termination, cookies kept out of persistence, same-key reauthorization and invalid quality rejection.

`tests/youtube_live.py` also passed against the public **Me at the zoo** YouTube video (`jNQXAC9IVRw`), producing a 474,599-byte file containing both audio and video. This was a real network extraction with the bundled Node/EJS runtime, followed by FFprobe validation. The former upstream test URL `BaW_jenozKc` was unavailable; it remains only a syntactic URL fixture in offline extension tests.

Brave tests use real extension content scripts and native messaging. Controlled pages exercise direct video playback and a YouTube-origin fixture verifies quality, playlist exclusion and same-document navigation. That fixture is separate from the real YouTube extraction test. Firefox Snap exercises the actual on-page button, native portal and Rust file transfer.

## What the transfer checks establish

Tests use a controlled localhost HTTP fixture and isolated runtime/data directories. They compare output SHA-256 digests with the original fixture bytes and exercise:

- Segmented range requests, servers ignoring ranges, unknown lengths and redirects.
- Mid-body disconnection and retry without duplicated/corrupted bytes.
- Empty files, Content-Disposition names, and malformed range rejection.
- Pause/resume and forced backend termination with durable checkpoint recovery.
- Changed validators requiring an explicit restart and collision-safe finalization.
- An interrupted finalization transaction recovered through the persisted whole-file digest.
- Expired authorization, cookie-authenticated transfers, credential loss on service restart, and browser reauthorization without persisting cookie secrets.
- Native-message framing, durable idempotency, rejection of administrative browser commands, queue gating, live subscriptions, private socket permissions and graceful shutdown.

The desktop tests drive the actual compiled Tauri app through tauri-driver 2.1.0 and WebKitGTK. Browser tests use real browser extension runtimes and the actual Rust native-messaging adapter; neither native messaging nor the transfer engine is mocked in these checks. Test profiles and native-host manifests are isolated/restored.

## Reproduce

```bash
cargo test --locked -p rdm-core -p rdm-protocol
cargo clippy --locked -p rdm-core -p rdm-backend -p rdm-protocol --all-targets -- -D warnings
npm test
npm run build
npm run extensions
cargo build --locked -p rdm-backend
python3 tests/acceptance.py
python3 tests/media_acceptance.py
python3 tests/firefox_stream_detection.py
# Optional external-site test:
python3 tests/youtube_live.py
node tests/browser_chromium.mjs
RDM_FIREFOX_INTERACTIVE=1 python3 tests/browser_firefox_snap.py
```

For runtime-only package checks:

```bash
docker build -t rdm-runtime:24.04 -f packaging/Dockerfile.runtime --build-arg UBUNTU_VERSION=24.04 .
docker build -t rdm-runtime:26.04 -f packaging/Dockerfile.runtime --build-arg UBUNTU_VERSION=26.04 .
docker run --rm --user 1000:1000 -v "$PWD:/workspace" rdm-runtime:24.04 dbus-run-session -- xvfb-run -a python3 tests/desktop_webdriver.py
docker run --rm --user 1000:1000 -v "$PWD:/workspace" rdm-runtime:26.04 dbus-run-session -- xvfb-run -a python3 tests/desktop_webdriver.py
```

The WebDriver executable lives at `.cache/driver/bin/tauri-driver`; the README gives its installation command. Ubuntu 26.04 renamed the test-driver package to `webkitgtk-webdriver`; the runtime Dockerfile selects the available package automatically.

## Limits

- Firefox Snap requires a focused browser for GNOME's native-host access dialog. A headless test cannot grant it. The interactive test passed after the user approved the dialog; the implementation does not pre-grant or bypass access.
- Firefox distribution uses a temporary add-on and needs reloading after browser restart. Permanent installation requires Mozilla signing.
- Ordinary transfers support HTTP/HTTPS GET. The video provider supports current yt-dlp extractors and HLS/DASH; DRM, live recording, POST-generated downloads, unsupported challenge-bound sessions, FTP, torrents and site grabbing are not included.
- Video resume uses provider-managed partials, rather than the Rust HTTP engine's validated byte ranges. Available formats and site authorization may change between attempts. Pause active videos before changing speed or global concurrency limits.
- Without a reliable representation validator, interrupted ordinary HTTP downloads restart instead of appending unsafe bytes. The UI identifies whether safe resume is available.
- Browser download cancellation and handoff are separate browser/service operations. The extension retains the browser transfer on a negative/unavailable native acknowledgement; successful handoffs are deduplicated.
- RDM was installed on the user’s Ubuntu 26.04 host with administrator authentication. Browser manifests are registered as the user. The package includes the user-service unit; first launch starts it on demand, and enabling login startup remains an explicit user choice.
- These checks establish the tested scenarios. They do not establish universal website compatibility or IDM-equivalent performance.

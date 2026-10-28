# Rust Download Manager

A local, GPL-3.0-only download manager for Ubuntu, with a Rust background service and a Tauri 2 / React desktop app. This is an original implementation; the proprietary installer in `build/` is a reference input and is never packaged.

## Work with coding agents

The public source is [MudabbirulSaad/rust-download-manager](https://github.com/MudabbirulSaad/rust-download-manager).
Locally, ask: **Use $rdm-implement-ticket to implement RDM-15.** In a fresh prepared
Codex Cloud task, ask: **Use $rdm-review-ticket to review RDM-15, fix in-scope issues,
run required checks, then merge and close only if every gate passes.**
See [the agent workflow](docs/agent-workflow.md) for setup, ticket readiness, exact
revision evidence and recovery after interrupted handoffs. `AGENTS.md` and the two
repository skills carry the same guidance into local and cloud work.
See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution and provenance rules.

## Install the Ubuntu build

Download/build `dist/rdm_0.2.2_amd64.deb`, then:

```bash
sudo apt install ./dist/rdm_0.2.2_amd64.deb
rdm
```

The package targets Ubuntu **24.04 and 26.04 amd64**. It installs runtime dependencies through apt; Rust, Node, npm, and development packages are not needed to run it. The package does not change browser profiles or enable startup services during installation.

Closing the desktop window leaves the Rust service running. Reopen `rdm` to reconnect. Use **Settings → Stop background service** to stop it. Starting at login is optional; downloads do not run after logout by default.

## Browser setup

1. Open **Connect your browser** in RDM and click **Register browser connection**.
2. Brave: open `brave://extensions`, turn on developer mode, click **Load unpacked**, and select `/usr/share/rdm/extensions/chromium`.
3. Firefox: open `about:debugging#/runtime/this-firefox`, click **Load Temporary Add-on**, and select `/usr/share/rdm/extensions/firefox/manifest.json`.
4. Open the extension popup and click **Test connection**. Firefox Snap may ask permission to start the native host through its portal/proxy.
5. Right-click an HTTP/HTTPS link and choose **Download with Rust Download Manager**.

For **Firefox Snap**, load the extension from a location visible inside the Snap
instead of `/usr/share/rdm`. Prepare it with:

```bash
mkdir -p "$HOME/snap/firefox/common/rdm-extension"
cp -a /usr/share/rdm/extensions/firefox/. "$HOME/snap/firefox/common/rdm-extension/"
```

In **Load Temporary Add-on**, select
`~/snap/firefox/common/rdm-extension/manifest.json`. Native-host registration still
runs on the host using `rdm-backend register-browsers`.

For signed-in downloads, visit the relevant site and click **Allow signed-in downloads on this site** in the extension popup before sending its links. Cookie access is optional; the video overlay uses content-script site access. Cookies are scoped to the destination and held only in service memory; re-send a link after credentials expire or the service restarts. Credentials are not sent across unrelated redirects.

Automatic capture is **off initially**. Enable it in the popup to capture known GET downloads from approved sites. Unknown request methods, POST responses, private downloads, `blob:` and `data:` transfers stay in the browser. A failed native handoff retains/resumes the browser's download.

Firefox's temporary add-on must be reloaded after restarting the browser. Permanent Firefox distribution needs Mozilla signing. Brave's development identity is pinned by the public key in `extensions/identity.json`. No signing private key is included or needed for local development.

When upgrading from 0.2.1 or earlier, pause active jobs, use **Settings → Stop background service**, install 0.2.2, then reopen RDM and reload the extensions. Existing history is retained. Approve signed-in sites again in the updated popup; overlay page access never counts as cookie approval. A video handoff to an older running service is rejected with a restart instruction.

## Video download buttons

Version **0.2.2** fixes blob/MediaSource players by using HLS/DASH manifests observed in the current video frame. Plain-text `playlist.txt`/`index.txt` streams are validated by their HLS contents after a click; unrelated text and image-named segments are never selected as complete videos. The popup also asks embedded video frames for their actual stream instead of sending the outer HTML page. Public cross-origin media context includes the originating site’s Origin header for CDNs that require it. The manifest URL and public request context are sent only when you click the overlay. Cookie access still requires explicit site approval. Reload the extension and the video tab after updating, start playback, then click the on-video button again.

If a player page is unsupported, the error is visible in its download row; click **Details** for recovery instructions. A visible button does not guarantee that a site’s authentication, DRM or browser-bound playback can be transferred.

Version **0.2.0** adds a **Download video** toolbar over visible HTML5 players, including YouTube watch pages, Shorts, embedded players and videos inserted during page navigation. Choose **Best**, 4K, 1440p, 1080p, 720p, 480p or 360p; this caps the selected height and falls back to available lower quality. Direct MP4/WebM links use the Rust HTTP engine. YouTube, HLS/DASH manifests and supported video pages use the pinned **yt-dlp** provider, with **FFmpeg** merging separate audio/video streams. The result can be MP4, WebM or MKV without transcoding.

Click the toolbar to enqueue the video and follow progress in RDM. The extension confirms only after durable acceptance. Repeated clicks on the same video/quality reuse a handoff key. Playback and the browser page are left intact. The overlay also has keyboard focus, a hide-for-this-page button and a global **Show download buttons on videos** switch in the popup. The popup's **Download video from this page** action is useful when a player hides extension controls. Container fullscreen is supported; native fullscreen of a bare video element cannot display HTML overlays, so use the popup or leave fullscreen.

Reload both the extension and existing video tabs after upgrading. The content script needs access to HTTP/HTTPS pages to find players; cookie access remains optional and site approved. No video is sent to the service until you click. For authenticated videos, approve the video site in the popup first. Browser cookies go through the native pipe into an in-memory cookie jar, with domain/path/secure and host-only rules; RDM does not read browser profile databases or write cookie files.

You can also paste a YouTube URL into **Add download**, or enable **Download videos from these pages** for other sites. Quality is selected in the same dialog. Video jobs participate in queues, scheduling, background operation and pause/resume. Their provider checkpoints are separate from Rust's validated HTTP byte ranges. **Restart** discards a video's provider partials. Pause active videos before changing their speed limits or global speed/concurrency settings. A global speed limit reserves a conservative equal share per configured download slot for each video.

The `.deb` includes a private Node runtime and the pinned yt-dlp/EJS scripts. It installs Python and FFmpeg through apt; no global Node or developer toolchain is required. The provider has no automatic code downloads, user configuration, browser cookie database access, plugins or self-update. Update `packaging/media-tools.json` and rebuild RDM when a site requires a newer provider.

DRM-protected players, active live broadcasts and some browser-bound/site challenge flows cannot be downloaded. A `blob:` player is resolved through its page when yt-dlp supports that site; a blob URL itself is never handed to the file downloader. Site compatibility and available qualities depend on the site's current responses.

## Build from source

Use Rust **1.99.1** (`rustup` reads `rust-toolchain.toml`) and Node **24.21.0** (`nvm use`).

On Ubuntu, install the Tauri prerequisites:

```bash
sudo apt update
sudo apt install build-essential pkg-config curl wget file libwebkit2gtk-4.1-dev \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf ffmpeg python3
npm ci
python3 scripts/fetch-media-tools.py
npm run types
cargo build -p rdm-backend
npm run tauri dev
```

Compiled frontend assets are isolated in `web-dist/`; release packages and notices are written to `dist/` to prevent recursive asset inclusion.

The development frontend listens only on `127.0.0.1:1420`. Its browser preview is visibly disconnected; transfers require the desktop app.

Create the complete release on Ubuntu 24.04:

Release packaging requires a clean Git checkout with the source changes committed.
Its source archive is generated from that exact commit; arbitrary workspace files
and generated outputs are excluded. A source archive can build the app, but create
a Git checkout before producing distribution packages from it.

```bash
npm run release
```

Or use the included Ubuntu 24.04 build container from a newer Ubuntu host:

```bash
bash scripts/container.sh bash scripts/release.sh
```

Docker and user-installed Rust/Node must already be available. Select the pinned Node version before using the container (`nvm use`). The container mounts their caches and the workspace, runs as your user, and limits compiler concurrency. All release files are written to `dist/`: the `.deb`, two extension ZIPs, license notices, and `SHA256SUMS`. `build/*.exe` is excluded from Docker context and packages.

For a source-tree service, the GUI finds `rdm-backend` beside its executable; alternatively set `RDM_BACKEND` to its absolute path. Installed builds start the `systemd --user` service on demand. Source-tree launches can start an independent backend when no unit is installed.

## Verify

```bash
cargo test --locked -p rdm-core -p rdm-protocol
npm test
npm run build
npm run extensions
cargo build --locked -p rdm-backend
python3 tests/acceptance.py
python3 tests/media_acceptance.py
# Optional real-network YouTube smoke test:
python3 tests/youtube_live.py
```

`tests/acceptance.py` runs a controlled local HTTP server and isolated service/data directories. It checks actual file hashes, range fallback, resume, SIGKILL recovery, changed validators, cookie authorization, native messaging, duplicate handoffs, queues, subscriptions, and private socket permissions. It never downloads from outside localhost or accesses your real download history.

Tauri WebDriver smoke tests run against the compiled application under Xvfb:

```bash
bash scripts/container.sh cargo install tauri-driver --version 2.1.0 --locked --root /workspace/.cache/driver
bash scripts/container.sh dbus-run-session -- xvfb-run -a python3 tests/desktop_webdriver.py
```

To test your real browsers in disposable profiles:

```bash
node tests/browser_chromium.mjs
RDM_FIREFOX_INTERACTIVE=1 python3 tests/browser_firefox_snap.py
```

The Firefox test requires the official geckodriver binary at `.cache/geckodriver/geckodriver` (tested version 0.37.1). It opens a disposable Firefox Snap window; click **Test connection** and allow RDM's local native host when the system asks. It restores the test manifest and removes the profile afterward. Existing browser profiles are not used. Browser-specific checks and release verification results are documented in `VERIFICATION.md`.

For diagnostics, `rdm-backend status` prints the current snapshot and `journalctl --user -u rdm.service` shows an installed service’s logs. Stop the background service before running `rdm-backend serve` manually.

## Architecture

- `crates/protocol`: versioned request/response and data types; generated TypeScript DTOs.
- `crates/core`: transfer engine, SQLite state, schedules, bandwidth control, client framing, and launch discovery.
- `crates/backend`: daemon, restricted browser native-host adapter, browser registration, and diagnostic CLI.
- `src-tauri`: desktop Rust adapter; no download logic in JavaScript.
- `src`: React UI, accessible dialogs, sortable table, theme, queue and service controls.
- `extensions`: shared browser code, video overlays and separate Chromium/Firefox manifests.
- `media/worker.py`: supervised yt-dlp helper; Rust controls its process group, queues, durable state, hashing and final file rename.

The daemon is the only owner of transfer and persistence state. The desktop uses a user-private Unix socket and half-second snapshot subscription. Browser messaging can enqueue downloads and check health; it cannot shut down the service or administer jobs.

SQLite state is under `$XDG_DATA_HOME/rdm` (normally `~/.local/share/rdm`). The socket is under `$XDG_RUNTIME_DIR/rdm`. Both private directories are mode 0700 and the socket/database are mode 0600. `RDM_DATA_DIR` and `RDM_SOCKET` allow isolated tests.

Temporary `.rdm-<id>.part` files live beside their destination for atomic finalization. Only synced ranges become durable checkpoints. Resume requires a matching strong validator and exact response ranges. Files without a usable validator use a single stream; unsafe resumes require an explicit restart. Existing destination files are never replaced. **Remove** clears history and retains downloaded files; cancelled partials can be explicitly restarted.

Development builds add content verification before finalization. New jobs persist
their expected content (`auto`, `binary`, `html`, or `media`), inferred from the
requested filename/source and response metadata. API callers can set the optional
`expected_content` field explicitly. A server-provided filename cannot turn an
expected binary download into an accepted HTML login page. Explicit HTML and
untyped page downloads remain supported; unknown file types retain ordinary GET
behavior. Existing history gains compatible defaults and completed history is
not retroactively verified.

Direct media and provider outputs must pass FFprobe container/stream checks that
read every packet before a digest can journal completion. The verifier has no
network/playlist access, bounded output, and a 30-second to 10-minute deadline
scaled by file size. It stops on pause or backend termination. Invalid/truncated
media remains in its private partial file with an actionable error; **Resume**
retries verification of a fully durable body, and **Restart** fetches it again.
This detects container/packet errors; it is not a supplied-checksum or semantic
content guarantee. Use a backend advertising `content-validation-v1`; stop an
older background service before testing the new build. Publication and installed
upgrades are separate from this development change.

Queues support daily local-time windows, including overnight windows. Missed windows wait for the next scheduled opening; suspended/logged-out sessions cannot transfer. Defaults: 3 downloads, 4 workers for large files, 8 connections per origin. A limit of zero means unlimited.

Media work files live in a private `.rdm-<id>.media/` directory beside the destination and are removed after successful completion. A whole-file digest journals the final atomic, non-overwriting rename for videos too.

The service stores source URLs, including any query tokens needed to resume. Keep the private data directory private. Cookie credentials remain in memory. Logs do not include URLs or cookie values.

## Boundaries

HTTP/HTTPS GET transfers, cookie-based authentication and supported video-page/HLS/DASH extraction are supported. DRM, live recording, unsupported browser-bound challenge flows, FTP, torrents, site grabbing, IDM data import, Windows/macOS and browser-store publication are outside this release. There is no claim of proprietary IDM internal-code or speed equivalence.

## License

GPL-3.0-only. See `LICENSE` and `THIRD_PARTY_NOTICES.md`. Original icons and frontend assets are included; all UI assets run locally.

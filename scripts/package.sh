#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
task_target="${CARGO_TARGET_DIR:-target/ubuntu24}"
task_stage="$(pwd)/dist/deb-root"
rm -rf "$task_stage"
mkdir -p "$task_stage"/{DEBIAN,usr/bin,usr/lib/rdm,usr/lib/systemd/user,usr/share/applications,usr/share/icons/hicolor/128x128/apps,usr/share/doc/rdm,usr/share/rdm/extensions}
install -m755 "$task_target/release/rdm" "$task_stage/usr/bin/rdm"
install -m755 "$task_target/release/rdm-backend" "$task_stage/usr/lib/rdm/rdm-backend"
mkdir -p "$task_stage/usr/lib/rdm/media"
install -m644 media/worker.py "$task_stage/usr/lib/rdm/media/worker.py"
install -m644 .cache/media/yt-dlp "$task_stage/usr/lib/rdm/media/yt-dlp"
install -m755 .cache/media/node "$task_stage/usr/lib/rdm/media/node"
cp packaging/media-tools.json "$task_stage/usr/lib/rdm/media/versions.json"
ln -s ../lib/rdm/rdm-backend "$task_stage/usr/bin/rdm-backend"
install -m644 packaging/rdm.service "$task_stage/usr/lib/systemd/user/rdm.service"
install -m644 packaging/rdm.desktop "$task_stage/usr/share/applications/rdm.desktop"
install -m644 src-tauri/icons/128x128.png "$task_stage/usr/share/icons/hicolor/128x128/apps/rdm.png"
cp -R extensions/dist/. "$task_stage/usr/share/rdm/extensions/"
cp LICENSE README.md THIRD_PARTY_NOTICES.md VERIFICATION.md "$task_stage/usr/share/doc/rdm/"
mkdir -p "$task_stage/usr/share/doc/rdm/media-licenses"
cp .cache/media/NODE-LICENSE.txt .cache/media/YT-DLP-LICENSE .cache/media/YT-DLP-THIRD_PARTY_LICENSES.txt "$task_stage/usr/share/doc/rdm/media-licenses/"
if [ -d dist/licenses ]; then cp -R dist/licenses "$task_stage/usr/share/doc/rdm/"; fi
cat > "$task_stage/DEBIAN/control" <<'CONTROL'
Package: rdm
Version: 0.2.2
Section: net
Priority: optional
Architecture: amd64
Maintainer: Rust Download Manager contributors
Depends: libwebkit2gtk-4.1-0, libgtk-3-0t64, libayatana-appindicator3-1, libxdo3, xdg-utils, dbus-user-session, python3 (>= 3.10), ffmpeg, ca-certificates
Description: Rust Download Manager desktop application and background service
 HTTP and HTTPS segmented downloads with safe resume, queues, schedules,
 browser native messaging, embedded video downloads and a React desktop interface.
CONTROL
# The package never enables a service or edits browser profiles as root.
dpkg-deb --root-owner-group --build "$task_stage" dist/.rdm_0.2.2_amd64.tmp.deb
mv dist/.rdm_0.2.2_amd64.tmp.deb dist/rdm_0.2.2_amd64.deb
python3 - <<'PY'
from pathlib import Path
import zipfile
for target in ['chromium','firefox']:
 with zipfile.ZipFile(f'dist/rdm-extension-{target}-0.2.2.zip','w',zipfile.ZIP_DEFLATED) as z:
  for p in Path(f'extensions/dist/{target}').rglob('*'):
   if p.is_file():z.write(p,p.relative_to(f'extensions/dist/{target}'))
PY
tar --exclude='./dist' --exclude='./web-dist' --exclude='./target' --exclude='./node_modules' --exclude='./build' --exclude='./.agents' --exclude='./.cache' --exclude='./.git' --exclude='*.tsbuildinfo' --exclude='*.pyc' --exclude='__pycache__' -czf dist/rdm-0.2.2-source.tar.gz .
(cd dist && sha256sum rdm_0.2.2_amd64.deb rdm-extension-*-0.2.2.zip rdm-0.2.2-source.tar.gz > SHA256SUMS)

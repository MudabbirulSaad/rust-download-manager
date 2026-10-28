#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
task_os=$(. /etc/os-release; printf '%s' "$VERSION_ID")
if [[ "$task_os" != 24.04 ]]; then
  exec bash scripts/container.sh bash scripts/agent-package.sh
fi
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}"
python3 scripts/fetch-media-tools.py
npm run build
npm run extensions
cargo build --locked --release -p rdm-backend -p rdm-desktop --features rdm-desktop/custom-protocol
python3 scripts/notices.py
bash scripts/package.sh

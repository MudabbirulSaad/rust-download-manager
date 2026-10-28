#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
task_mode="${1:-build}"
task_os=$(. /etc/os-release; printf '%s' "$VERSION_ID")
if [[ "$task_os" != 24.04 ]]; then
  exec bash scripts/container.sh bash scripts/agent-desktop.sh "$task_mode"
fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}"
task_target=$(realpath -m "$CARGO_TARGET_DIR")
export RDM_MEDIA_DIR="$PWD/.cache/media"
case "$task_mode" in
  build)
    cargo build --locked --release -p rdm-backend -p rdm-desktop --features rdm-desktop/custom-protocol
    ;;
  test)
    export RDM_DESKTOP="$task_target/release/rdm"
    export RDM_TEST_BACKEND="$task_target/release/rdm-backend"
    export RDM_SCREENSHOT_DIR="$PWD/.agent-runs/desktop-screenshots"
    exec dbus-run-session -- xvfb-run -a python3 tests/desktop_webdriver.py
    ;;
  *) printf 'Use build or test\n' >&2; exit 2 ;;
esac

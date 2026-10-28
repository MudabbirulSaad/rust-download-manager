#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
npm ci
python3 scripts/fetch-media-tools.py
npm run types
npm test
npm run build
npm run extensions
cargo test --locked -p rdm-core -p rdm-protocol
cargo build --locked --release -p rdm-backend -p rdm-desktop --features rdm-desktop/custom-protocol
RDM_TEST_BACKEND="${CARGO_TARGET_DIR:-target}/release/rdm-backend" python3 tests/acceptance.py
RDM_TEST_BACKEND="${CARGO_TARGET_DIR:-target}/release/rdm-backend" python3 tests/media_acceptance.py
python3 scripts/notices.py
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}" bash scripts/package.sh

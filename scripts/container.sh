#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
task_root="$(pwd)"
task_node="$(dirname "$(dirname "$(command -v node)")")"
docker build -q -t rdm-builder:ubuntu24 -f packaging/Dockerfile .
exec docker run --rm --user "$(id -u):$(id -g)" --memory=3g --cpus=2 \
 -v "$task_root:/workspace" \
 -v "$HOME/.cargo:/home/mudabbir/.cargo" \
 -v "$HOME/.rustup:/home/mudabbir/.rustup" \
 -v "$task_node:/opt/node:ro" \
 -e CARGO_TARGET_DIR=/workspace/target/ubuntu24 -e CARGO_BUILD_JOBS=1 \
 rdm-builder:ubuntu24 "$@"

#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
task_root="$(pwd)"
task_node="$(dirname "$(dirname "$(command -v node)")")"
task_git_mount=()
task_git_common=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)
# Managed worktrees point .git at their primary checkout. Keep read-only access
# to that metadata at its original path so provenance checks work in /workspace.
if [[ -n "$task_git_common" && "$task_git_common" != "$task_root/.git" ]]; then
  task_git_mount=(-v "$task_git_common:$task_git_common:ro" -e GIT_OPTIONAL_LOCKS=0)
fi
docker build -q -t rdm-builder:ubuntu24 -f packaging/Dockerfile .
exec docker run --rm --user "$(id -u):$(id -g)" --memory=3g --cpus=2 \
 -v "$task_root:/workspace" \
 -v "$HOME/.cargo:/home/mudabbir/.cargo" \
 -v "$HOME/.rustup:/home/mudabbir/.rustup" \
 -v "$task_node:/opt/node:ro" \
 "${task_git_mount[@]}" \
 -e CARGO_TARGET_DIR=/workspace/target/ubuntu24 -e CARGO_BUILD_JOBS=1 \
 rdm-builder:ubuntu24 "$@"

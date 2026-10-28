#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
task_version=$(node -p 'JSON.parse(require("fs").readFileSync("package.json","utf8")).version')
task_sha=$(git rev-parse HEAD)
task_runtime_uid=$(id -u)
task_runtime_gid=$(id -g)
# A root caller still exercises the installed app as an ordinary user.
if ((task_runtime_uid == 0)); then task_runtime_uid=1000; task_runtime_gid=1000; fi
mkdir -p .agent-runs/package-matrix
for task_ubuntu in 24.04 26.04; do
  task_screenshots="$PWD/.agent-runs/package-matrix/$task_ubuntu/screenshots"
  if [[ -L "$task_screenshots" ]]; then printf 'Screenshot directory must not be a symlink\n' >&2; exit 1; fi
  mkdir -p "$task_screenshots"
  if ((EUID == 0)); then chown "$task_runtime_uid:$task_runtime_gid" "$task_screenshots"; fi
  task_image="rdm-agent-runtime:$task_ubuntu-${task_sha:0:12}"
  docker build -t "$task_image" -f packaging/Dockerfile.runtime \
    --build-arg "UBUNTU_VERSION=$task_ubuntu" --build-arg "RDM_VERSION=$task_version" .
  docker run --rm --user "$task_runtime_uid:$task_runtime_gid" \
    --tmpfs /rdm-tiny:rw,size=1m,mode=0777 \
    -v "$PWD:/workspace:ro" -v "$task_screenshots:/artifacts" \
    -e RDM_MEDIA_DIR=/usr/lib/rdm/media \
    -e RDM_SCREENSHOT_DIR=/artifacts \
    "$task_image" python3 tests/installed_acceptance.py
  docker run --rm --user 0:0 -v "$PWD:/workspace:ro" \
    -e "RDM_CANDIDATE_DEB=/workspace/dist/rdm_${task_version}_amd64.deb" \
    "$task_image" python3 tests/package_lifecycle.py
done

#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
task_system=0
task_drivers=0
for task_arg in "$@"; do
  case "$task_arg" in
    --system) task_system=1 ;;
    --drivers) task_drivers=1 ;;
    *) printf 'Unknown setup option: %s\n' "$task_arg" >&2; exit 2 ;;
  esac
done
if ((task_system)); then
  task_apt=(apt-get)
  if ((EUID != 0)); then task_apt=(sudo apt-get); fi
  "${task_apt[@]}" update
  task_webdriver=webkit2gtk-driver
  if ! apt-cache show "$task_webdriver" >/dev/null 2>&1; then task_webdriver=webkitgtk-webdriver; fi
  "${task_apt[@]}" install -y --no-install-recommends build-essential pkg-config curl wget file \
    ca-certificates libwebkit2gtk-4.1-dev libxdo-dev libssl-dev \
    libayatana-appindicator3-dev librsvg2-dev patchelf ffmpeg python3 \
    xvfb xauth dbus-x11 "$task_webdriver" fonts-dejavu-core
fi
if [[ -f "$HOME/.cargo/env" ]]; then source "$HOME/.cargo/env"; fi
if ! command -v rustup >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rdm-rustup-init.sh
  bash /tmp/rdm-rustup-init.sh -y --profile minimal --default-toolchain none
  source "$HOME/.cargo/env"
fi
task_rust=$(sed -n 's/^channel = "\([^"]*\)"/\1/p' rust-toolchain.toml)
rustup toolchain install "$task_rust" --profile minimal --component rustfmt --component clippy
python3 scripts/fetch-media-tools.py
task_node=$(tr -d '\n\r' < .nvmrc)
if [[ "$(node --version 2>/dev/null || true)" != "v$task_node" ]]; then
  mkdir -p .cache/toolchains/node
  tar -xJf ".cache/media/node-v$task_node-linux-x64.tar.xz" -C .cache/toolchains/node --strip-components=1
  export PATH="$PWD/.cache/toolchains/node/bin:$PATH"
fi
cat > .cache/agent-env.sh <<'ENV'
# Source this file at task start after the published environment is restored.
task_agent_cache=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
if [[ -x "$task_agent_cache/toolchains/node/bin/node" ]]; then
  export PATH="$task_agent_cache/toolchains/node/bin:$HOME/.cargo/bin:$PATH"
else
  export PATH="$HOME/.cargo/bin:$PATH"
fi
export RDM_MEDIA_DIR="$task_agent_cache/media"
unset task_agent_cache
ENV
npm ci
if ((task_drivers)) && [[ ! -x .cache/driver/bin/tauri-driver ]]; then
  cargo install tauri-driver --version 2.1.0 --locked --root .cache/driver
fi
npm run agent:doctor
npm run agent:validate
printf '\nSetup passed. At task start: source .cache/agent-env.sh\n'

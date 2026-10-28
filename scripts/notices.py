#!/usr/bin/env python3
from pathlib import Path
import json,tomllib,shutil
root=Path(__file__).resolve().parent.parent
out=root/'dist/licenses';out.mkdir(parents=True,exist_ok=True)
lines=['# Third-party notices','','RDM is GPL-3.0-only. Dependencies retain their own licenses. Bundled license texts are in `licenses/` in the installed documentation.','','## Rust dependencies','']
lock=tomllib.loads((root/'Cargo.lock').read_text())
registry=Path.home()/'.cargo/registry/src'
for item in lock['package']:
 if 'source' not in item:continue
 name,version=item['name'],item['version']
 found=list(registry.glob(f'*/{name}-{version}/Cargo.toml'))
 if not found:continue # Other-platform dependencies are not in this Linux build.
 pkg=found[0].parent;meta=tomllib.loads(found[0].read_text())['package']
 lines.append(f'- {name} {version}: {meta.get("license","see upstream license file")} — [source package](https://crates.io/api/v1/crates/{name}/{version}/download)')
 for f in pkg.iterdir():
  if f.is_file() and f.name.lower().startswith(('license','copying','notice')):
   target=out/f'rust-{name}-{version}';target.mkdir(exist_ok=True);shutil.copyfile(f,target/f.name)
lines+=['','## Frontend runtime dependencies','']
packages=json.loads((root/'package-lock.json').read_text())['packages']
for path,meta in packages.items():
 if not path or meta.get('dev'):continue
 pkg=root/path
 if not (pkg/'package.json').exists():continue
 info=json.loads((pkg/'package.json').read_text());name=info['name'];version=info['version']
 lines.append(f'- {name} {version}: {info.get("license","see upstream license file")} — [source package]({meta.get("resolved", "https://www.npmjs.com/package/"+name)})')
 for f in pkg.iterdir():
  if f.is_file() and f.name.lower().startswith(('license','copying','notice')):
   target=out/f'npm-{name.replace("/","-")}-{version}';target.mkdir(exist_ok=True);shutil.copyfile(f,target/f.name)
lines += ['', '## Media tools', '',
 '- yt-dlp 2026.10.26 (Unlicense), including its bundled EJS scripts and dependency notices. [Source and release](https://github.com/yt-dlp/yt-dlp/releases/tag/2026.10.26).',
 '- Node.js 24.21.0 (MIT and bundled third-party licenses). [Source and release](https://nodejs.org/dist/v24.21.0/). The private runtime is used only by the media provider.',
 '- FFmpeg and Python are Ubuntu package dependencies, supplied by apt with their distribution license/source information.',
 '', 'Full bundled tool licenses are installed in `/usr/share/doc/rdm/media-licenses/`. Executable versions and checksums are pinned in `packaging/media-tools.json`.']
(root/'THIRD_PARTY_NOTICES.md').write_text('\n'.join(lines)+'\n')
print('Generated third-party notices and license texts.')

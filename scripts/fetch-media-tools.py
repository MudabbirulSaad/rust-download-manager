#!/usr/bin/env python3
"""Fetch pinned upstream media tools, verifying every executable before use."""
import hashlib
import json
import tarfile
import urllib.request
from pathlib import Path

root = Path(__file__).resolve().parents[1]
cache = root / '.cache/media'
cache.mkdir(parents=True, exist_ok=True)
for tool in json.loads((root / 'packaging/media-tools.json').read_text()):
    archive = cache / tool['url'].rsplit('/', 1)[-1]
    if not archive.exists() or hashlib.sha256(archive.read_bytes()).hexdigest() != tool['sha256']:
        data = urllib.request.urlopen(tool['url'], timeout=120).read()
        if hashlib.sha256(data).hexdigest() != tool['sha256']:
            raise SystemExit(f"Checksum mismatch for {tool['name']}")
        archive.write_bytes(data)
    if tool['name'] == 'node':
        prefix = archive.name.removesuffix('.tar.xz')
        with tarfile.open(archive, 'r:xz') as tar:
            (cache / 'node').write_bytes(tar.extractfile(prefix + '/bin/node').read())
            (cache / 'NODE-LICENSE.txt').write_bytes(tar.extractfile(prefix + '/LICENSE').read())
        (cache / 'node').chmod(0o755)
    else:
        archive.chmod(0o755)
        for name in ['LICENSE', 'THIRD_PARTY_LICENSES.txt']:
            url = f"https://raw.githubusercontent.com/yt-dlp/yt-dlp/{tool['version']}/{name}"
            (cache / ('YT-DLP-' + name)).write_bytes(urllib.request.urlopen(url, timeout=30).read())
    print(f"Verified {tool['name']} {tool['version']}")

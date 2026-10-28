#!/usr/bin/env python3
"""Fetch pinned upstream media tools, verifying every executable before use."""
import hashlib
import json
import subprocess
import tarfile
import time
from pathlib import Path


def fetch_bytes(url, timeout=120, retries=2):
    # A per-socket read timeout can be reset by a slowly trickling response.
    # Bound the entire transfer and the curl process, including name resolution.
    for attempt in range(retries + 1):
        print(f"Fetching {url.rsplit('/', 1)[-1]} (attempt {attempt + 1}/{retries + 1})", flush=True)
        try:
            return subprocess.run([
                'curl', '--fail', '--location', '--silent', '--show-error',
                '--connect-timeout', str(min(20, timeout)),
                '--max-time', str(timeout), url,
            ], check=True, stdout=subprocess.PIPE, timeout=timeout + 25).stdout
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
            if attempt == retries:
                raise
            # Each attempt has a fresh output buffer; partial bodies cannot be
            # concatenated into a later successful response.
            time.sleep(min(2 ** attempt, 4))
    raise AssertionError('Unreachable download attempt')


def main():
    root = Path(__file__).resolve().parents[1]
    cache = root / '.cache/media'
    cache.mkdir(parents=True, exist_ok=True)
    for tool in json.loads((root / 'packaging/media-tools.json').read_text()):
        archive = cache / tool['url'].rsplit('/', 1)[-1]
        if not archive.exists() or hashlib.sha256(archive.read_bytes()).hexdigest() != tool['sha256']:
            data = fetch_bytes(tool['url'])
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
                (cache / ('YT-DLP-' + name)).write_bytes(fetch_bytes(url, timeout=30))
        print(f"Verified {tool['name']} {tool['version']}", flush=True)


if __name__ == '__main__':
    main()

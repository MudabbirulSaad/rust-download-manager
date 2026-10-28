#!/usr/bin/env python3
"""Verify same-version reinstall/removal preserves private history and output.

This deliberately says reinstall, not a historical-version upgrade. Run only in a
disposable runtime container, as root; no host service or user data is involved.
"""
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
from http.server import ThreadingHTTPServer
from acceptance import Fixture, SHA, framed, receive

assert os.geteuid() == 0 and Path('/.dockerenv').exists(), 'Disposable Docker root only'
binary = '/usr/lib/rdm/rdm-backend'
server = ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
threading.Thread(target=server.serve_forever, daemon=True).start()
with tempfile.TemporaryDirectory(prefix='rdm-package-life-') as temporary:
    root = Path(temporary)
    env = {**os.environ, 'RDM_DATA_DIR': str(root/'data'), 'RDM_SOCKET': str(root/'run.sock')}
    def request(command):
        with socket.socket(socket.AF_UNIX) as sock:
            sock.settimeout(20)
            sock.connect(env['RDM_SOCKET'])
            sock.sendall(framed({'version': 1, 'id': 'lifecycle', 'command': command}))
            result = receive(sock)
            assert result['ok'], result
            return result['data']
    def start():
        child = subprocess.Popen([binary, 'serve'], env=env, stdout=subprocess.DEVNULL)
        for _ in range(200):
            try:
                request({'type': 'health'})
                return child
            except OSError:
                time.sleep(.05)
        child.terminate()
        raise AssertionError('Installed service did not start')
    child = start()
    try:
        identifier = request({'type': 'add', 'spec': {'url': f'http://127.0.0.1:{server.server_port}/file', 'destination': str(root/'files')}})['id']
        for _ in range(400):
            job = next(j for j in request({'type': 'snapshot'})['downloads'] if j['id'] == identifier)
            if job['status'] == 'completed':
                break
            time.sleep(.05)
        assert job['status'] == 'completed', job
    finally:
        request({'type': 'shutdown'})
        child.wait(timeout=20)
    output = Path(job['destination'])/job['filename']
    assert hashlib.sha256(output.read_bytes()).hexdigest() == SHA
    subprocess.run(['apt-get', 'install', '-y', '--reinstall', os.environ['RDM_CANDIDATE_DEB']], check=True)
    child = start()
    try:
        recovered = next(j for j in request({'type': 'snapshot'})['downloads'] if j['id'] == identifier)
        assert recovered['status'] == 'completed' and recovered['sha256'] == SHA
    finally:
        request({'type': 'shutdown'})
        child.wait(timeout=20)
    subprocess.run(['apt-get', 'remove', '-y', 'rdm'], check=True)
    assert (root/'data/state.db').is_file()
    assert hashlib.sha256(output.read_bytes()).hexdigest() == SHA
    print('PASS installed package reinstall and removal preserve private history and exact downloaded bytes', flush=True)
server.shutdown()

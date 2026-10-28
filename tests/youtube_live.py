#!/usr/bin/env python3
"""Optional real-network smoke test; site challenges can prevent unattended use."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

binary = Path(os.environ.get('RDM_TEST_BACKEND', 'target/debug/rdm-backend')).resolve()
url = os.environ.get('RDM_YOUTUBE_TEST_URL', 'https://www.youtube.com/watch?v=jNQXAC9IVRw')
with tempfile.TemporaryDirectory(prefix='rdm-youtube-') as temp:
    env = {**os.environ, 'RDM_DATA_DIR': temp+'/data', 'RDM_SOCKET': temp+'/service.sock'}
    service = subprocess.Popen([str(binary), 'serve'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    def request(command):
        return json.loads(subprocess.check_output([str(binary), 'request', json.dumps(command)], env=env, stderr=subprocess.DEVNULL))
    try:
        for _ in range(100):
            try: request({'type':'health'}); break
            except subprocess.CalledProcessError: time.sleep(.1)
        id = request({'type':'add_media','spec':{'url':url,'destination':temp+'/files','media':{'max_height':720}}})['id']
        for _ in range(360):
            job = next(j for j in request({'type':'snapshot'})['downloads'] if j['id']==id)
            if job['status']=='completed':
                streams=json.loads(subprocess.check_output(['ffprobe','-v','quiet','-show_streams','-of','json',str(Path(job['destination'],job['filename']))]))
                assert {'audio','video'} <= {s['codec_type'] for s in streams['streams']}
                print('PASS real YouTube page -> Rust service -> playable audio/video:',job['filename'],job['total'],job['sha256']); break
            if job['status']=='needs_attention':
                print('UNAVAILABLE real YouTube test:',job['error']); raise SystemExit(2)
            time.sleep(.5)
        else: raise SystemExit('Real YouTube test timed out')
    finally:
        try: request({'type':'shutdown'})
        except subprocess.CalledProcessError: pass
        service.wait(timeout=15)

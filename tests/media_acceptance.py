#!/usr/bin/env python3
"""Real yt-dlp + FFmpeg + Rust service tests using generated, local media."""
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import struct
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

binary = Path(os.environ.get('RDM_TEST_BACKEND', 'target/debug/rdm-backend')).resolve()


def main():
    with tempfile.TemporaryDirectory(prefix='rdm-media-') as temp:
        root = Path(temp)
        fixtures = root / 'fixtures'
        fixtures.mkdir()
        subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-f', 'lavfi', '-i',
                        'testsrc2=size=640x360:rate=24', '-f', 'lavfi', '-i',
                        'sine=frequency=660:sample_rate=44100', '-t', '6', '-c:v', 'mpeg4',
                        '-q:v', '3', '-c:a', 'aac', '-movflags', '+faststart', str(fixtures / 'video.mp4')], check=True)
        subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-i', str(fixtures / 'video.mp4'),
                        '-c:v', 'libx264', '-preset', 'ultrafast', '-c:a', 'aac', '-f', 'dash',
                        '-seg_duration', '2', str(fixtures / 'manifest.mpd')], check=True)
        (fixtures / 'hls').mkdir()
        subprocess.run(['ffmpeg','-hide_banner','-loglevel','error','-i',str(fixtures/'video.mp4'),
                        '-c:v','libx264','-preset','ultrafast','-c:a','aac','-f','hls','-hls_time','2',
                        '-hls_playlist_type','vod','-hls_segment_filename',str(fixtures/'hls/seg_%05d.png'),
                        str(fixtures/'hls/index.txt')],check=True)
        (fixtures/'hls/playlist.txt').write_text('#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000000,RESOLUTION=640x360\nindex.txt\n')
        original = (fixtures / 'video.mp4').read_bytes()

        class Fixture(BaseHTTPRequestHandler):
            seen = []
            referrers = []
            origins = []
            def log_message(self, *_args): pass
            def do_GET(self):
                path = self.path.split('?')[0]
                Fixture.seen.append((path, self.headers.get('Cookie')))
                Fixture.referrers.append((path, self.headers.get('Referer')))
                Fixture.origins.append((path,self.headers.get('Origin')))
                if path.startswith('/hls/') and self.headers.get('Origin') != page_origin:
                    self.send_response(403);self.end_headers();return
                if path == '/unsupported-page':
                    body=b'<html><title>Custom player fixture</title><div id="player"></div></html>'
                    self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body);return
                if path in ('/referer-page', '/referer.mp4') and self.headers.get('Referer') not in (base+'/',base+'/referer-page'):
                    self.send_response(403);self.end_headers();return
                if path == '/auth-page' and self.headers.get('Cookie') != 'session=approved':
                    self.send_response(401); self.end_headers(); return
                if path.endswith('page'):
                    source = '/manifest.mpd' if path == '/dash-page' else '/slow.mp4' if path == '/slow-page' else '/referer.mp4' if path == '/referer-page' else '/video.mp4'
                    body = f'<html><title>RDM local video</title><video controls src="{source}"></video></html>'.encode()
                    self.send_response(200); self.send_header('Content-Type', 'text/html'); self.send_header('Content-Length', str(len(body))); self.end_headers(); self.wfile.write(body); return
                file = fixtures / ('video.mp4' if path in ('/slow.mp4','/referer.mp4') else path.lstrip('/'))
                if not file.is_file(): self.send_response(404); self.end_headers(); return
                data = file.read_bytes(); start, end = 0, len(data)-1
                ranged = self.headers.get('Range', '')
                if ranged:
                    first, last = ranged.removeprefix('bytes=').split('-'); start = int(first); end = min(int(last) if last else end, end)
                self.send_response(206 if ranged else 200)
                if ranged: self.send_header('Content-Range', f'bytes {start}-{end}/{len(data)}')
                self.send_header('Content-Length', str(end-start+1)); self.send_header('Accept-Ranges', 'bytes')
                self.send_header('Content-Type', 'application/dash+xml' if file.suffix == '.mpd' else 'text/plain' if file.suffix=='.txt' else 'image/png' if file.suffix=='.png' else 'video/mp4'); self.end_headers()
                try:
                    for offset in range(start, end+1, 16384):
                        self.wfile.write(data[offset:min(offset+16384, end+1)])
                        if path == '/slow.mp4': time.sleep(.03)
                except (BrokenPipeError, ConnectionResetError): pass

        server = ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f'http://127.0.0.1:{server.server_port}'
        page_origin=f'http://localhost:{server.server_port}'
        env = {**os.environ, 'RDM_SOCKET': str(root / 'service.sock'), 'RDM_DATA_DIR': str(root / 'data')}
        proc = None
        def request(command, fail=False):
            data = json.dumps({'version': 1, 'id': 'media-test', 'command': command}).encode()
            with socket.socket(socket.AF_UNIX) as stream:
                stream.settimeout(15); stream.connect(env['RDM_SOCKET']); stream.sendall(struct.pack('<I', len(data))+data)
                def exact(n):
                    value = b''
                    while len(value) < n:
                        part = stream.recv(n-len(value)); assert part; value += part
                    return value
                value = json.loads(exact(struct.unpack('<I', exact(4))[0]))
            if fail: assert not value['ok']; return value
            assert value['ok'], value; return value['data']
        def start():
            nonlocal proc
            proc = subprocess.Popen([str(binary), 'serve'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            for _ in range(100):
                try: request({'type': 'health'}); return
                except (OSError, AssertionError): time.sleep(.1)
            raise AssertionError('Media service did not start')
        def job(id): return next(j for j in request({'type': 'snapshot'})['downloads'] if j['id'] == id)
        def wait(id, predicate, timeout=60):
            deadline = time.monotonic()+timeout
            while time.monotonic() < deadline:
                value = job(id)
                if predicate(value): return value
                if value['status'] == 'needs_attention': raise AssertionError(value)
                time.sleep(.05)
            raise AssertionError(job(id))
        def add(path, **extra):
            spec = {'url': base+path, 'destination': str(root / 'files'), 'media': {'max_height': 1080}, **extra}
            return request({'type': 'add_media', 'spec': spec})['id']
        try:
            start()
            id = add('/page', idempotency_key='media-fixture')
            done = wait(id, lambda j: j['status'] == 'completed')
            assert Path(done['destination'], done['filename']).read_bytes() == original
            assert done['sha256'] == hashlib.sha256(original).hexdigest()
            assert done['downloaded'] == done['total'] == len(original)
            assert add('/page', idempotency_key='media-fixture') == id
            print('PASS media page extraction, exact hash, title and idempotent acknowledgement', flush=True)

            id = add('/dash-page')
            done = wait(id, lambda j: j['status'] == 'completed')
            output = Path(done['destination'], done['filename'])
            assert output.suffix == '.mkv'
            probe = json.loads(subprocess.check_output(['ffprobe', '-v', 'quiet', '-show_streams', '-of', 'json', str(output)]))
            assert {'video', 'audio'} <= {s['codec_type'] for s in probe['streams']}
            print('PASS actual DASH video/audio streams merged by FFmpeg into a playable file', flush=True)

            id=add('/hls/playlist.txt',context={'cookies':[],'referer':page_origin+'/player?token=private','user_agent':'RDM fixture'})
            done=wait(id,lambda j:j['status']=='completed')
            output=Path(done['destination'],done['filename'])
            probe=json.loads(subprocess.check_output(['ffprobe','-v','quiet','-show_streams','-of','json',str(output)]))
            assert {'video','audio'} <= {s['codec_type'] for s in probe['streams']}
            assert all(origin==page_origin for path,origin in Fixture.origins if path.startswith('/hls/'))
            assert all('private' not in (ref or '') for _,ref in Fixture.referrers)
            print('PASS plain-text HLS with image-named TS fragments and required public Origin produces playable audio/video',flush=True)

            id = add('/slow-page')
            wait(id, lambda j: j['downloaded'] > 200000 and j['status'] == 'running')
            request({'type': 'control', 'ids': [id], 'action': 'pause'})
            wait(id, lambda j: j['status'] == 'paused' and j['active_connections'] == 0)
            assert not Path(job(id)['destination'], job(id)['filename']).exists()
            request({'type': 'control', 'ids': [id], 'action': 'resume'})
            wait(id, lambda j: j['downloaded'] > 400000 and j['status'] == 'running')
            proc.kill(); proc.wait(); time.sleep(.5); start()
            done = wait(id, lambda j: j['status'] == 'completed')
            assert Path(done['destination'], done['filename']).read_bytes() == original
            assert any(path == '/slow.mp4' for path, _ in Fixture.seen)
            print('PASS video pause/resume and forced service termination recover exact bytes', flush=True)

            context = {'cookies': [{'name': 'session', 'value': 'approved', 'domain': '127.0.0.1', 'path': '/', 'secure': False, 'host_only': True}]}
            id = add('/auth-page', context=context)
            done = wait(id, lambda j: j['status'] == 'completed')
            assert Path(done['destination'], done['filename']).read_bytes() == original
            assert any(path == '/auth-page' and cookie == 'session=approved' for path, cookie in Fixture.seen)
            assert b'approved' not in (root / 'data/downloads.sqlite3').read_bytes()
            assert not list((root / 'files').glob('*.cookies'))
            print('PASS approved media cookies stay out of history and files', flush=True)
            id = add('/slow-page', context=context, idempotency_key='auth-media-restart')
            wait(id, lambda j: j['downloaded'] > 200000 and j['status'] == 'running')
            proc.kill(); proc.wait(); time.sleep(.5); start()
            assert job(id)['status'] == 'needs_attention'
            assert add('/slow-page', context=context, idempotency_key='auth-media-restart') == id
            done = wait(id, lambda j: j['status'] == 'completed')
            assert Path(done['destination'], done['filename']).read_bytes() == original
            print('PASS same-key browser reauthorization resumes a video after service restart', flush=True)
            id = add('/referer-page', context={'cookies': [], 'referer':base+'/player?token=secret','user_agent':'RDM regression test'})
            done = wait(id, lambda j:j['status']=='completed')
            assert Path(done['destination'],done['filename']).read_bytes()==original
            assert any(path=='/referer.mp4' and ref in (base+'/',base+'/referer-page') for path,ref in Fixture.referrers)
            assert all('secret' not in (ref or '') for _,ref in Fixture.referrers)
            print('PASS public video context satisfies hotlink checks without leaking page tokens',flush=True)
            id = add('/unsupported-page')
            failed = wait(id,lambda j:j['status']=='needs_attention')
            assert 'player page is unsupported' in failed['error']
            assert 'Play the video' in failed['error'] and failed['downloaded']==0
            print('PASS unsupported player pages explain stream capture recovery',flush=True)
            request({'type': 'add', 'spec': {'url': base+'/page', 'media': {'max_height': 999}}}, fail=True)
            print('PASS invalid video quality rejected', flush=True)
        finally:
            if proc and proc.poll() is None:
                try: request({'type': 'shutdown'})
                except OSError: pass
                proc.wait(timeout=15)
            server.shutdown()


if __name__ == '__main__': main()

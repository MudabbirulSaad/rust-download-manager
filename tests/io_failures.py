#!/usr/bin/env python3
"""Run inside a container with /rdm-tiny (1MiB tmpfs) and /rdm-denied (root-owned 0555)."""
import os,json,tempfile,subprocess,socket,threading,time
from pathlib import Path
from http.server import ThreadingHTTPServer
from acceptance import Fixture,framed,receive
binary=Path(os.environ['RDM_TEST_BACKEND']).resolve()
server=ThreadingHTTPServer(('127.0.0.1',0),Fixture);threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='rdm-io-')as temp:
 env={**os.environ,'RDM_DATA_DIR':temp+'/data','RDM_SOCKET':temp+'/service.sock'}
 proc=subprocess.Popen([str(binary),'serve'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 def command(c):
  with socket.socket(socket.AF_UNIX)as s:s.settimeout(10);s.connect(env['RDM_SOCKET']);s.sendall(framed({'version':1,'id':'io','command':c}));r=receive(s);assert r['ok'],r;return r['data']
 try:
  for _ in range(100):
   try:command({'type':'health'});break
   except OSError:time.sleep(.1)
  for directory,expected in [('/rdm-denied','Permission denied'),('/rdm-tiny','No space left')]:
   id=command({'type':'add','spec':{'url':f'http://127.0.0.1:{server.server_port}/file','destination':directory}})['id']
   deadline=time.time()+40
   while time.time()<deadline:
    job=next(j for j in command({'type':'snapshot'})['downloads']if j['id']==id)
    if job['status']=='needs_attention':break
    time.sleep(.1)
   assert job['status']=='needs_attention'and expected in job['error'],job
   assert not(Path(directory)/job['filename']).exists()
   print('PASS',expected,'leaves an actionable error and no completed output',flush=True)
 finally:
  try:command({'type':'shutdown'})
  except Exception:pass
  proc.wait(timeout=15);server.shutdown()

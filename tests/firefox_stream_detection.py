#!/usr/bin/env python3
"""Real Snap content-script regression; no native-host/profile changes."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time
import urllib.request
import zipfile
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer

root=Path(__file__).resolve().parents[1]
class Page(BaseHTTPRequestHandler):
    def log_message(self,*args):pass
    def do_GET(self):
        manifest=self.path=='/manifest.mpd'
        body=b'<MPD/>' if manifest else b'<!doctype html><title>RDM blob detection fixture</title><video width="720" height="405" controls></video><script>document.querySelector("video").src=URL.createObjectURL(new Blob(["fixture"],{type:"video/mp4"}));fetch("/manifest.mpd")</script>'
        self.send_response(200);self.send_header('Content-Type','application/dash+xml' if manifest else 'text/html');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)

def main():
    temp=Path(tempfile.mkdtemp(prefix='rdm-stream-',dir=Path.home()/'snap/firefox/common'))
    profile=temp/'profile';profile.mkdir();(profile/'user.js').write_text('user_pref("marionette.port",2843);\n')
    log=(temp/'driver.log').open('wb');browser=None;driver=None;session=None
    server=ThreadingHTTPServer(('127.0.0.1',0),Page);threading.Thread(target=server.serve_forever,daemon=True).start()
    def api(method,path,body=None):
        req=urllib.request.Request('http://127.0.0.1:4449'+path,data=None if body is None else json.dumps(body).encode(),method=method,headers={'Content-Type':'application/json'})
        with urllib.request.urlopen(req,timeout=30) as r:return json.load(r)['value']
    try:
        addon=temp/'addon.xpi'
        with zipfile.ZipFile(addon,'w',zipfile.ZIP_DEFLATED) as z:
            for p in (root/'extensions/dist/firefox').iterdir():
                if p.is_file():z.write(p,p.name)
        browser=subprocess.Popen(['/snap/bin/firefox','--headless','--no-remote','--marionette','--profile',str(profile)],stdout=log,stderr=log)
        driver=subprocess.Popen([str(root/'.cache/geckodriver/geckodriver'),'--port','4449','--connect-existing','--marionette-port','2843','--allow-system-access'],stdout=log,stderr=log)
        for _ in range(150):
            try:api('GET','/status');break
            except OSError:time.sleep(.1)
        session=api('POST','/session',{'capabilities':{'alwaysMatch':{'browserName':'firefox'}}})['sessionId']
        api('POST',f'/session/{session}/moz/addon/install',{'path':str(addon),'temporary':True})
        api('POST',f'/session/{session}/url',{'url':f'http://127.0.0.1:{server.server_port}/player'})
        title=''
        for _ in range(100):
            title=api('POST',f'/session/{session}/execute/sync',{'script':'return document.querySelector("rdm-video-toolbar")?.shadowRoot.querySelector("#download")?.title??""','args':[]})
            if 'Download detected video from 127.0.0.1' in title:break
            time.sleep(.1)
        assert 'Download detected video from 127.0.0.1' in title,title
        print('PASS actual Firefox Snap content script detects a fetched manifest behind a blob player',flush=True)
    except Exception:
        print((temp/'driver.log').read_text(errors='replace'));raise
    finally:
        if session:
            try:api('DELETE',f'/session/{session}')
            except Exception:pass
        if driver:
            driver.terminate()
            try:driver.wait(timeout=10)
            except subprocess.TimeoutExpired:driver.kill()
        if browser and browser.poll() is None:
            try:browser.terminate();browser.wait(timeout=5)
            except PermissionError:subprocess.run(['snap','run','--shell','firefox','-c',f'kill {browser.pid}'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15)
            except subprocess.TimeoutExpired:pass
        server.shutdown();log.close();shutil.rmtree(temp,ignore_errors=True)

if __name__=='__main__':main()

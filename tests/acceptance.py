#!/usr/bin/env python3
"""Offline integration checks against the actual backend executable; no external downloads."""
import hashlib,json,os,socket,struct,subprocess,tempfile,threading,time,sys,sqlite3
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from pathlib import Path

BINARY=Path(os.environ.get('RDM_TEST_BACKEND','target/debug/rdm-backend')).resolve()
DATA=bytes(range(256))*(80*1024)+b'rdm-end'
SHA=hashlib.sha256(DATA).hexdigest()
class Fixture(BaseHTTPRequestHandler):
    changed=False
    seen=[]
    dropped=False
    def log_message(self,*args):pass
    def do_GET(self):
        path=self.path.split('?')[0]
        Fixture.seen.append((path,self.headers.get('Range'),self.headers.get('Cookie')))
        if path=='/empty':
            self.send_response(416);self.send_header('Content-Range','bytes */0');self.send_header('Content-Length','0');self.end_headers();return
        if path=='/auth' and self.headers.get('Cookie')!='session=approved':
            self.send_response(401);self.send_header('Content-Length','0');self.end_headers();return
        if path=='/redirect':
            self.send_response(302);self.send_header('Location','/file');self.send_header('Content-Length','0');self.end_headers();return
        start,end=0,len(DATA)-1
        ranged=self.headers.get('Range') and path not in ['/no-range','/unknown']
        if ranged:
            first,last=self.headers['Range'][6:].split('-')
            start,end=int(first),int(last) if last else len(DATA)-1
            if start>=len(DATA):self.send_response(416);self.end_headers();return
            end=min(end,len(DATA)-1)
        self.send_response(206 if ranged else 200)
        if path!='/unknown':self.send_header('Content-Length',str(end-start+1))
        if ranged:self.send_header('Content-Range',f'bytes {start+1 if path=="/malformed" else start}-{end}/{len(DATA)}')
        if path=='/disposition':self.send_header('Content-Disposition','attachment; filename="fixture.zip"')
        self.send_header('ETag','"version2"' if Fixture.changed else '"version1"')
        self.end_headers()
        try:
            if path=='/drop-once' and ranged and end>start and not Fixture.dropped:
                Fixture.dropped=True;self.wfile.write(DATA[start:start+128*1024]);return
            for offset in range(start,end+1,64*1024):
                self.wfile.write(DATA[offset:min(offset+64*1024,end+1)])
                if path in ['/slow','/auth']:time.sleep(.008)
        except (BrokenPipeError,ConnectionResetError):pass

def framed(value):
    body=json.dumps(value).encode();return struct.pack('<I',len(body))+body

def receive(stream):
    def exact(n):
        value=b''
        while len(value)<n:
            part=stream.recv(n-len(value));assert part,'Unexpected service EOF';value+=part
        return value
    return json.loads(exact(struct.unpack('<I',exact(4))[0]))

def main():
    server=ThreadingHTTPServer(('127.0.0.1',0),Fixture)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    base=f'http://127.0.0.1:{server.server_port}'
    passed=[]
    with tempfile.TemporaryDirectory(prefix='rdm-acceptance-') as temp:
        root=Path(temp);env={**os.environ,'RDM_DATA_DIR':str(root/'data'),'RDM_SOCKET':str(root/'run/service.sock'),'RDM_BACKEND':str(BINARY)}
        log=(root/'service.log').open('wb')
        proc=None
        def start():
            nonlocal proc
            proc=subprocess.Popen([str(BINARY),'serve'],env=env,stdout=log,stderr=log)
            deadline=time.time()+10
            while time.time()<deadline:
                try:request({'type':'health'});return
                except (OSError,AssertionError):time.sleep(.05)
            raise AssertionError('Service did not start: '+(root/'service.log').read_text())
        def request(command):
            with socket.socket(socket.AF_UNIX) as s:
                s.settimeout(15);s.connect(env['RDM_SOCKET']);s.sendall(framed({'version':1,'id':'test','command':command}));response=receive(s)
                assert response['ok'],response.get('error');return response['data']
        def snapshot():return request({'type':'snapshot'})
        def job(id):return next(j for j in snapshot()['downloads'] if j['id']==id)
        def wait(id,predicate,timeout=30):
            deadline=time.time()+timeout
            while time.time()<deadline:
                value=job(id)
                if predicate(value):return value
                time.sleep(.05)
            raise AssertionError(f'Timeout waiting for {id}: {job(id)}')
        def add(path,**kwargs):
            spec={'url':base+path,'destination':str(root/'files'),'queue_id':'main',**kwargs}
            return request({'type':'add','spec':spec})['id']
        def complete(id):
            j=wait(id,lambda j:j['status'] in ['completed','needs_attention','failed'])
            assert j['status']=='completed',j['error']
            actual=hashlib.sha256((Path(j['destination'])/j['filename']).read_bytes()).hexdigest()
            assert actual==SHA and j['sha256']==SHA,(actual,j)
            return j
        def control(id,action):return request({'type':'control','ids':[id],'action':action})
        def record(name):passed.append(name);print('PASS',name,flush=True)
        try:
            start()
            id=add('/file');complete(id)
            assert len([r for r in Fixture.seen if r[0]=='/file' and r[1]])>=5
            record('segmented transfer and SHA-256 integrity')
            complete(add('/drop-once'));assert Fixture.dropped;record('mid-body interruption retries without corrupting bytes')
            j=complete(add('/disposition'));assert j['filename']=='fixture.zip';record('server filename detection')
            id=add('/empty');j=wait(id,lambda j:j['status'] in ['completed','needs_attention']);assert j['status']=='completed' and j['total']==0;assert (Path(j['destination'])/j['filename']).read_bytes()==b'';record('empty range resource')
            for path in ['/no-range','/unknown','/redirect']:
                complete(add(path));record(path+' transfer integrity')
            id=add('/malformed');j=wait(id,lambda j:j['status']=='needs_attention');assert 'range' in j['error'].lower();record('malformed ranges rejected')
            id=add('/slow',filename='pause.bin');wait(id,lambda j:len(j['completed_ranges'])>=1)
            control(id,'pause');wait(id,lambda j:j['active_connections']==0 and j['status']=='paused')
            # The daemon's active cleanup can finish one scheduler tick after connection counters.
            time.sleep(.3);control(id,'resume');complete(id);record('pause and safe resume')
            id=add('/slow',filename='crash.bin');wait(id,lambda j:len(j['completed_ranges'])>=1)
            proc.kill();proc.wait();time.sleep(.1);start();complete(id);record('SIGKILL and durable checkpoint recovery')
            id=add('/slow',filename='changed.bin');wait(id,lambda j:len(j['completed_ranges'])>=1)
            control(id,'pause');wait(id,lambda j:j['active_connections']==0);time.sleep(.3);Fixture.changed=True
            control(id,'resume');j=wait(id,lambda j:j['status']=='needs_attention');assert 'changed' in j['error'].lower();Fixture.changed=False
            time.sleep(.3);control(id,'restart');complete(id);record('changed validators require explicit restart')
            first=add('/file',filename='collision.bin');complete(first);second=add('/file',filename='collision.bin');complete(second)
            assert job(first)['filename']!=job(second)['filename'];record('existing files are not overwritten')
            id=add('/auth');wait(id,lambda j:j['status']=='needs_attention');record('expired authorization is actionable')
            ctx={'cookies':[{'name':'session','value':'approved','domain':'127.0.0.1','path':'/','secure':False,'host_only':True}],'referer':None,'user_agent':'RDM test'}
            id=add('/auth',context=ctx,idempotency_key='auth-test');complete(id)
            dup=add('/auth',context=ctx,idempotency_key='auth-test');assert id==dup;record('cookie handoff and durable idempotency')
            id=add('/auth',context=ctx,filename='auth-refresh.bin');wait(id,lambda j:len(j['completed_ranges'])>=1)
            proc.kill();proc.wait();start();assert job(id)['status']=='needs_attention'
            db=sqlite3.connect(root/'data/downloads.sqlite3');raw=db.execute("SELECT value FROM state WHERE key='snapshot'").fetchone()[0];db.close();assert 'approved' not in raw
            refreshed=add('/auth',context=ctx);assert refreshed==id;complete(id);record('authorization stays in memory and is refreshed after restart')
            wire=framed({'version':1,'id':'native','command':{'type':'add','spec':{'url':base+'/file','destination':str(root/'files'),'idempotency_key':'native-1'}}})
            native=subprocess.run([str(BINARY),'native-host'],input=wire,capture_output=True,env=env,timeout=15)
            assert native.returncode==0,native.stderr
            length=struct.unpack('<I',native.stdout[:4])[0];response=json.loads(native.stdout[4:4+length]);assert response['ok'],response
            complete(response['data']['id']);record('actual native-messaging adapter handoff')
            forbidden=subprocess.run([str(BINARY),'native-host'],input=framed({'version':1,'id':'bad','command':{'type':'shutdown'}}),capture_output=True,env=env,timeout=10)
            response=json.loads(forbidden.stdout[4:]);assert not response['ok'];request({'type':'health'});record('browser host rejects administrative commands')
            q={'id':'night','name':'Night queue','concurrency':1,'enabled':False,'start_time':None,'stop_time':None}
            request({'type':'upsert_queue','queue':q});id=add('/file',queue_id='night');time.sleep(.5);assert job(id)['status']=='queued'
            q['enabled']=True;request({'type':'upsert_queue','queue':q});complete(id);record('queue gating and restart')
            with socket.socket(socket.AF_UNIX) as s:
                s.settimeout(10);s.connect(env['RDM_SOCKET']);s.sendall(framed({'version':1,'id':'events','command':{'type':'subscribe'}}));assert receive(s)['data']['downloads'];assert receive(s)['ok']
            record('live snapshot subscription')
            assert (root/'data').stat().st_mode&0o077==0 and (root/'run/service.sock').stat().st_mode&0o077==0
            record('private runtime paths')
            request({'type':'shutdown'});proc.wait(timeout=15);record('graceful background service shutdown')
            db=sqlite3.connect(root/'data/downloads.sqlite3');state=json.loads(db.execute("SELECT value FROM state WHERE key='snapshot'").fetchone()[0]);entry=next(j for j in state['downloads'] if j['status']=='completed' and not j['authenticated']);entry['status']='running';recover_id=entry['id'];db.execute("UPDATE state SET value=? WHERE key='snapshot'",(json.dumps(state),));db.commit();db.close()
            start();complete(recover_id);record('finalization journal recovers a rename before completion commit')
        finally:
            if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=15)
            log.close();server.shutdown()
    print(f'{len(passed)} acceptance checks passed.')
if __name__=='__main__':main()

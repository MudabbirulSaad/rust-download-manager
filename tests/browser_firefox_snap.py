#!/usr/bin/env python3
"""Real Firefox Snap temporary extension / native messaging smoke test.
Uses a disposable Snap-visible profile; restores the native manifest afterward.
"""
import json,os,subprocess,tempfile,time,urllib.request,urllib.error,zipfile,shlex,threading,hashlib,shutil
from pathlib import Path
from http.server import ThreadingHTTPServer
from acceptance import Fixture,SHA
ROOT=Path(__file__).resolve().parent.parent
BACKEND=Path(os.environ.get('RDM_TEST_BACKEND',ROOT/'target/debug/rdm-backend')).resolve()
ELEMENT='element-6066-11e4-a52e-4f735466cecf'
UUID='73ca37b9-704b-4bae-aa35-b25d05f4b681'
def main():
 base=Path.home()/'snap/firefox/common';base.mkdir(parents=True,exist_ok=True)
 temp=Path(tempfile.mkdtemp(prefix='rdm-test-',dir=base));manifest=Path.home()/'.mozilla/native-messaging-hosts/org.rdm.download_manager.json';backup=manifest.read_bytes()if manifest.exists()else None
 env={**os.environ,'RDM_DATA_DIR':str(temp/'data'),'RDM_SOCKET':str(temp/'service.sock'),'RDM_BACKEND':str(BACKEND)}
 log=(temp/'webdriver.log').open('wb');driver=None;browser_proc=None;session=None
 class VideoFixture(Fixture):
  def do_GET(self):
   if self.path=='/video-page':
    body=b'<!doctype html><title>Firefox RDM video fixture</title><h1>Embedded video</h1><video width="720" height="405" controls src="/video.mp4"></video>'
    self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body);return
   super().do_GET()
 server=ThreadingHTTPServer(('127.0.0.1',0),VideoFixture);threading.Thread(target=server.serve_forever,daemon=True).start()
 def api(method,path,body=None):
  req=urllib.request.Request('http://127.0.0.1:4447'+path,data=None if body is None else json.dumps(body).encode(),method=method,headers={'Content-Type':'application/json'})
  try:
   with urllib.request.urlopen(req,timeout=60)as r:value=json.load(r)['value']
  except urllib.error.HTTPError as e:raise AssertionError(e.read().decode())from e
  assert not(isinstance(value,dict)and value.get('error')),value;return value
 def execute(script,args=[]):return api('POST',f'/session/{session}/execute/sync',{'script':script,'args':args})
 try:
  wrapper=temp/'native-host';wrapper.write_text('#!/bin/sh\n'+''.join(f'export {k}={shlex.quote(v)}\n'for k,v in env.items()if k.startswith('RDM_'))+'exec '+shlex.quote(str(BACKEND))+' native-host "$@"\n');wrapper.chmod(0o700)
  manifest.parent.mkdir(parents=True,exist_ok=True);manifest.write_text(json.dumps({'name':'org.rdm.download_manager','description':'RDM isolated integration test','type':'stdio','path':str(wrapper),'allowed_extensions':['rdm@download-manager.local']}))
  addon=temp/'extension.xpi'
  with zipfile.ZipFile(addon,'w',zipfile.ZIP_DEFLATED)as z:
   for f in (ROOT/'extensions/dist/firefox').rglob('*'):
    if f.is_file():z.write(f,f.relative_to(ROOT/'extensions/dist/firefox'))
  profile=temp/'profile';profile.mkdir()
  (profile/'user.js').write_text('user_pref("marionette.port", 2839);\nuser_pref("extensions.webextensions.uuids", '+json.dumps(json.dumps({'rdm@download-manager.local':UUID}))+');\n')
  headed=os.environ.get('RDM_FIREFOX_INTERACTIVE')=='1'
  browser_proc=subprocess.Popen(['/snap/bin/firefox',*(()if headed else('--headless',)),'--no-remote','--marionette','--profile',str(profile)],env=env,stdout=log,stderr=log)
  driver=subprocess.Popen([str(ROOT/'.cache/geckodriver/geckodriver'),'--port','4447','--connect-existing','--marionette-port','2839','--allow-system-access'],env=env,stdout=log,stderr=log)
  for _ in range(200):
   try:api('GET','/status');break
   except OSError:time.sleep(.1)
  session=api('POST','/session',{'capabilities':{'alwaysMatch':{'browserName':'firefox'}}})['sessionId']
  api('POST',f'/session/{session}/moz/addon/install',{'path':str(addon),'temporary':True})
  api('POST',f'/session/{session}/url',{'url':f'moz-extension://{UUID}/popup.html'})
  deadline=time.time()+20
  while time.time()<deadline:
   if execute('return !!document.querySelector("#test")'):break
   time.sleep(.1)
  element=api('POST',f'/session/{session}/element',{'using':'css selector','value':'#test'})
  if not headed:api('POST',f'/session/{session}/element/{element[ELEMENT]}/click',{})
  else:print('ACTION: In the temporary Firefox window, click Test connection and allow the local native host.',flush=True)
  deadline=time.time()+(240 if headed else 30);status=''
  while time.time()<deadline:
   status=execute('return document.querySelector("#result").textContent')
   if 'Connected to' in status or ('failed' in status and not headed):break
   time.sleep(.2)
  if 'Connected to the Rust background service.' not in status:
   detail=api('POST',f'/session/{session}/execute/async',{'script':"const done=arguments[arguments.length-1];browser.runtime.sendNativeMessage('org.rdm.download_manager',{version:1,id:'debug',command:{type:'health'}}).then(done,e=>done({error:String(e)}));",'args':[]})
   raise AssertionError('Firefox Snap native connection: '+status+' '+str(detail))
  print('PASS Firefox Snap extension connects through native messaging proxy',flush=True)
  result=api('POST',f'/session/{session}/execute/async',{'script':'''const done=arguments[arguments.length-1];browser.runtime.sendNativeMessage('org.rdm.download_manager',{version:1,id:'firefox-test',command:{type:'add',spec:{url:arguments[0],destination:arguments[1],idempotency_key:'firefox-test',context:{cookies:[{name:'session',value:'approved',domain:'127.0.0.1',path:'/',secure:false,host_only:true}],referer:null,user_agent:navigator.userAgent}}}}).then(done,e=>done({ok:false,error:String(e)}));''','args':[f'http://127.0.0.1:{server.server_port}/auth',str(temp/'files')]})
  assert result['ok'],result
  for _ in range(200):
   state=json.loads(subprocess.check_output([str(BACKEND),'status'],env=env,text=True));job=next(j for j in state['downloads']if j['id']==result['data']['id'])
   if job['status']=='completed':break
   time.sleep(.1)
  assert job['status']=='completed',job
  assert hashlib.sha256((Path(job['destination'])/job['filename']).read_bytes()).hexdigest()==SHA
  print('PASS Firefox Snap authenticated handoff downloads exact bytes',flush=True)
  api('POST',f'/session/{session}/url',{'url':f'http://127.0.0.1:{server.server_port}/video-page'})
  for _ in range(100):
   if execute('return !!document.querySelector("rdm-video-toolbar")?.shadowRoot.querySelector("#download")'):break
   time.sleep(.1)
  execute('document.querySelector("rdm-video-toolbar").shadowRoot.querySelector("#download").click()')
  for _ in range(100):
   status=execute('return document.querySelector("rdm-video-toolbar").shadowRoot.querySelector("[role=status]").textContent')
   if 'Added to RDM.' in status:break
   time.sleep(.1)
  assert 'Added to RDM.' in status,status
  for _ in range(200):
   state=json.loads(subprocess.check_output([str(BACKEND),'status'],env=env,text=True));video=next((j for j in state['downloads']if j['url'].endswith('/video.mp4')),None)
   if video and video['status']=='completed':break
   time.sleep(.1)
  assert video and video['status']=='completed',video
  assert hashlib.sha256((Path(video['destination'])/video['filename']).read_bytes()).hexdigest()==SHA
  print('PASS real Firefox Snap video overlay -> portal/native host -> Rust transfer exact hash',flush=True)
 except Exception:
  print((temp/'webdriver.log').read_text(errors='replace'));raise
 finally:
  if session:
   try:
    try:
     api('POST',f'/session/{session}/moz/context',{'context':'chrome'})
     execute('Services.startup.quit(Ci.nsIAppStartup.eForceQuit); return true;')
    except Exception:pass
    api('DELETE',f'/session/{session}/window')
    api('DELETE',f'/session/{session}')
   except Exception:pass
  if driver:
   try:driver.terminate();driver.wait(timeout=15)
   except (PermissionError,subprocess.TimeoutExpired):pass
  if browser_proc and browser_proc.poll() is None:
   try:browser_proc.terminate();browser_proc.wait(timeout=10)
   except PermissionError:
    subprocess.run(['snap','run','--shell','firefox','-c',f'kill {browser_proc.pid}'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15)
   except subprocess.TimeoutExpired:pass
  try:subprocess.run([str(BACKEND),'request','{"type":"shutdown"}'],env=env,capture_output=True,timeout=10)
  except Exception:pass
  if backup is None:manifest.unlink(missing_ok=True)
  else:manifest.write_bytes(backup)
  log.close();server.shutdown();shutil.rmtree(temp,ignore_errors=True)
if __name__=='__main__':main()

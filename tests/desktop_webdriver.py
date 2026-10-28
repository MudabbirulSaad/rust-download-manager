#!/usr/bin/env python3
"""Drive the real Tauri/WebKitGTK binary using tauri-driver's W3C protocol."""
import base64,json,os,subprocess,tempfile,time,urllib.request,urllib.error,threading,socket,struct,hashlib
from pathlib import Path
from http.server import ThreadingHTTPServer
from acceptance import Fixture,DATA,SHA
ROOT=Path(__file__).resolve().parent.parent
BINARY=Path(os.environ.get('RDM_DESKTOP',str(ROOT/'target/ubuntu24/release/rdm'))).resolve()
BACKEND=Path(os.environ.get('RDM_TEST_BACKEND',str(BINARY.with_name('rdm-backend')))).resolve()
DRIVER=ROOT/'.cache/driver/bin/tauri-driver'
ELEMENT='element-6066-11e4-a52e-4f735466cecf'
class DesktopFixture(Fixture):
 def do_GET(self):
  if self.path=='/unsupported-player':
   body=b'<html><title>Custom player fixture</title><div id="player"></div></html>'
   self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body);return
  super().do_GET()

def main():
 with tempfile.TemporaryDirectory(prefix='rdm-desktop-')as temp:
  root=Path(temp);env={**os.environ,'RDM_DATA_DIR':str(root/'data'),'RDM_SOCKET':str(root/'service.sock'),'RDM_BACKEND':str(BACKEND),'HOME':str(root),'WEBKIT_DISABLE_COMPOSITING_MODE':'1'}
  log=(root/'driver.log').open('wb');driver=subprocess.Popen([str(DRIVER),'--port','4445','--native-port','4446'],env=env,stdout=log,stderr=log)
  session=None
  def api(method,path,body=None):
   data=None if body is None else json.dumps(body).encode();req=urllib.request.Request('http://127.0.0.1:4445'+path,data=data,method=method,headers={'Content-Type':'application/json'})
   try:
    with urllib.request.urlopen(req,timeout=40)as r:result=json.load(r)
   except urllib.error.HTTPError as e:raise AssertionError(e.read().decode())from e
   value=result.get('value');assert not(isinstance(value,dict)and value.get('error')),value;return value
  def execute(script):return api('POST',f'/session/{session}/execute/sync',{'script':script,'args':[]})
  def wait(script,timeout=25):
   deadline=time.time()+timeout
   while time.time()<deadline:
    if execute(script):return
    time.sleep(.15)
   raise AssertionError('UI condition did not become true: '+script+'\n'+str(execute('return document.body.innerText')))
  def click(selector):
   el=api('POST',f'/session/{session}/element',{'using':'css selector','value':selector})
   api('POST',f'/session/{session}/element/{el[ELEMENT]}/click',{})
  def type_into(selector,text):
   el=api('POST',f'/session/{session}/element',{'using':'css selector','value':selector})
   api('POST',f'/session/{session}/element/{el[ELEMENT]}/value',{'text':text,'value':list(text)})
  def new_session():return api('POST','/session',{'capabilities':{'alwaysMatch':{'browserName':'wry','tauri:options':{'application':str(BINARY)}}}})['sessionId']
  def backend(command):
   out=subprocess.run([str(BACKEND),'request',json.dumps(command)],env=env,capture_output=True,text=True,timeout=20);assert out.returncode==0,out.stderr;return json.loads(out.stdout)
  fixture=ThreadingHTTPServer(('127.0.0.1',0),DesktopFixture);threading.Thread(target=fixture.serve_forever,daemon=True).start()
  try:
   for _ in range(100):
    try:api('GET','/status');break
    except (OSError,AssertionError):time.sleep(.1)
   session=new_session()
   wait("return document.body.innerText.includes('Service connected')")
   assert execute('const r=document.querySelector(".sidebar-bottom").getBoundingClientRect(); return r.bottom <= window.innerHeight && r.top >= 0;'), 'Sidebar service controls must remain visible'
   print('PASS actual Tauri application connects to Rust service',flush=True)
   click('.add-button');type_into('textarea',f'http://127.0.0.1:{fixture.server_port}/slow');click('.modal-footer button[type=submit]')
   wait("return document.querySelectorAll('tbody tr').length===1")
   # Close the actual GUI session while its independent service keeps downloading.
   api('DELETE',f'/session/{session}');session=None
   deadline=time.time()+30
   while time.time()<deadline:
    state=backend({'type':'snapshot'})
    if state['downloads'][0]['status']=='completed':break
    time.sleep(.1)
   job=state['downloads'][0];assert job['status']=='completed',job
   assert hashlib.sha256((Path(job['destination'])/job['filename']).read_bytes()).hexdigest()==SHA
   print('PASS closing all GUI windows leaves transfers running with correct hash',flush=True)
   session=new_session();wait("return document.body.innerText.includes('Service connected') && document.querySelectorAll('tbody tr').length===1")
   click('button[aria-label^="Details for"]');wait("return document.body.innerText.includes('SHA-256')")
   print('PASS reopening GUI reconnects and displays persisted transfer details',flush=True)
   media_id=backend({'type':'add_media','spec':{'url':f'http://127.0.0.1:{fixture.server_port}/unsupported-player','destination':str(root/'files'),'media':{'max_height':0}}})['id']
   deadline=time.time()+30
   while time.time()<deadline:
    failed=next(j for j in backend({'type':'snapshot'})['downloads']if j['id']==media_id)
    if failed['status']=='needs_attention':break
    time.sleep(.1)
   assert failed['status']=='needs_attention',failed
   wait('return !!document.querySelector(".row-error")')
   click('.row-error')
   wait('return document.querySelector(".error-text")?.textContent.includes("Play the video")')
   print('PASS native desktop exposes unsupported-player errors and recovery details',flush=True)
   screenshots=Path(os.environ.get('RDM_SCREENSHOT_DIR',str(ROOT/'dist/screenshots')));screenshots.mkdir(parents=True,exist_ok=True)
   for theme in ['light','dark']:
    if theme=='dark':click('button[aria-label="Use dark theme"]');wait("return document.documentElement.dataset.theme==='dark'")
    value=api('GET',f'/session/{session}/screenshot');(screenshots/f'rdm-{theme}.png').write_bytes(base64.b64decode(value))
   print('PASS light/dark screenshots and accessible controls',flush=True)
  except Exception:
   print((root/'driver.log').read_text(errors='replace'));raise
  finally:
   if session:
    try:api('DELETE',f'/session/{session}')
    except Exception:pass
   try:backend({'type':'shutdown'})
   except Exception:pass
   driver.terminate();driver.wait(timeout=10);log.close();fixture.shutdown()
if __name__=='__main__':main()

/** Real unpacked extension -> native host -> service test, isolated from user profiles. */
import {chromium} from 'playwright';
import {mkdtemp,mkdir,readFile,writeFile,rm,readdir} from 'node:fs/promises';
import{tmpdir}from'node:os';import{resolve,join}from'node:path';import{execFileSync}from'node:child_process';import{createServer}from'node:http';
const root=resolve('.');const temp=await mkdtemp(join(tmpdir(),'rdm-browser-'));const backend=resolve(process.env.RDM_TEST_BACKEND??'target/debug/rdm-backend');
const env={...process.env,HOME:temp,RDM_DATA_DIR:join(temp,'data'),RDM_SOCKET:join(temp,'service.sock'),RDM_BACKEND:backend};
const data=process.env.RDM_BROWSER_FIXTURE?await readFile(process.env.RDM_BROWSER_FIXTURE):Buffer.from('download integration fixture\n'.repeat(1000));
const streamDirectory=join(temp,'stream');await mkdir(streamDirectory);
execFileSync('ffmpeg',['-hide_banner','-loglevel','error','-f','lavfi','-i','testsrc2=size=320x180:rate=12','-f','lavfi','-i','sine=frequency=660:sample_rate=44100','-t','2','-c:v','libx264','-preset','ultrafast','-c:a','aac','-f','dash','-seg_duration','1',join(streamDirectory,'manifest.mpd')]);
const streams=new Map(await Promise.all((await readdir(streamDirectory)).map(async name=>[name,await readFile(join(streamDirectory,name))])));
const hlsDirectory=join(streamDirectory,'hls');await mkdir(hlsDirectory);
execFileSync('ffmpeg',['-hide_banner','-loglevel','error','-f','lavfi','-i','testsrc2=size=320x180:rate=12','-f','lavfi','-i','sine=frequency=660:sample_rate=44100','-t','2','-c:v','libx264','-preset','ultrafast','-c:a','aac','-f','hls','-hls_time','1','-hls_playlist_type','vod','-hls_segment_filename',join(hlsDirectory,'seg_%05d.png'),join(hlsDirectory,'index.txt')]);
await writeFile(join(hlsDirectory,'playlist.txt'),'#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000000,RESOLUTION=320x180\nindex.txt\n');
for(const name of await readdir(hlsDirectory))streams.set('hls/'+name,await readFile(join(hlsDirectory,name)));
const server=createServer((req,res)=>{
 if(req.url==='/plain-outer'){res.setHeader('Content-Type','text/html');return res.end('<!doctype html><title>Embedded player regression</title><iframe src="/plain-player" width="800" height="500"></iframe>');}
 if(req.url==='/plain-player'){res.setHeader('Content-Type','text/html');return res.end('<!doctype html><title>Plain-text HLS regression</title><video width="720" height="405" controls></video><script>document.querySelector("video").src=URL.createObjectURL(new Blob(["test"],{type:"video/mp4"}));fetch("/stream/hls/playlist.txt");fetch("/stream/hls/index.txt")</script>');}
 if(req.url==='/blob-page'){res.setHeader('Content-Type','text/html');return res.end('<!doctype html><title>Blob player regression</title><video width="720" height="405" controls></video><script>document.querySelector("video").src=URL.createObjectURL(new Blob(["test"],{type:"video/mp4"}));fetch("/stream/manifest.mpd")</script>');}
 if(req.url?.startsWith('/stream/')){
  if(!req.headers.referer?.startsWith(origin+'/')){res.writeHead(403);return res.end();}
  const name=req.url.slice('/stream/'.length).split('?')[0],body=streams.get(name);
  if(!body){res.writeHead(404);return res.end();}
  res.setHeader('Content-Type',name.endsWith('.mpd')?'application/dash+xml':name.endsWith('.txt')?'text/plain':name.endsWith('.png')?'image/png':'video/mp4');res.setHeader('Content-Length',body.length);return res.end(body);
 }
 if(req.url==='/auth'&&req.headers.cookie!=='session=approved'){res.writeHead(401,{'Content-Length':0});return res.end();}
 if(req.url==='/page'){res.setHeader('Content-Type','text/html; charset=utf-8');return res.end('<!doctype html><title>RDM video overlay test</title><style>body{margin:48px;font:16px system-ui;background:#e8edf4;color:#17243b}video{display:block;background:#111827;width:720px;height:405px}h1{font-size:24px}</style><h1>Embedded video · RDM integration test</h1><video controls src="/video.mp4" width="720" height="405"></video><a href="/auth">File</a>');}
 const range=req.headers.range;let start=0,end=data.length-1;if(range){const[first,last]=range.slice(6).split('-');start=Number(first);end=Math.min(last?Number(last):end,data.length-1);res.statusCode=206;res.setHeader('Content-Range',`bytes ${start}-${end}/${data.length}`);}
 if(req.url==='/video.mp4')res.setHeader('Content-Type','video/mp4');
 res.setHeader('ETag','"browser-fixture"');res.setHeader('Content-Length',end-start+1);res.end(data.subarray(start,end+1));
});await new Promise(r=>server.listen(0,'127.0.0.1',r));const origin=`http://127.0.0.1:${server.address().port}`;
let context;
try{
 await mkdir(join(temp,'Downloads'),{recursive:true});execFileSync(backend,['register-browsers'],{env});
 const profile=join(temp,'profile');await mkdir(join(profile,'NativeMessagingHosts'),{recursive:true});
 const manifest=JSON.parse(await readFile(join(temp,'.config/BraveSoftware/Brave-Browser/NativeMessagingHosts/org.rdm.download_manager.json'),'utf8'));
 await writeFile(join(profile,'NativeMessagingHosts/org.rdm.download_manager.json'),JSON.stringify(manifest));
 context=await chromium.launchPersistentContext(profile,{headless:true,executablePath:process.env.RDM_BROWSER??'/usr/bin/brave-browser',env,args:[`--disable-extensions-except=${root}/extensions/dist/chromium`,`--load-extension=${root}/extensions/dist/chromium`,'--no-first-run','--disable-gpu']});
 const id=(await readFile('extensions/chromium-id.txt','utf8')).trim();
 const popup=await context.newPage();await popup.goto(`chrome-extension://${id}/popup.html`);await popup.getByRole('button',{name:'Test connection'}).click();
 await popup.getByRole('status').filter({hasText:'Connected to the Rust background service.'}).waitFor({timeout:20000});
 console.log('PASS real Brave extension connects through native host');
 const page=await context.newPage();await page.goto(origin+'/page');
 // Exercise the same shared handoff operation used by the extension's context menu.
 // This is extension-context code, not website code or a mocked native bridge.
 const worker=context.serviceWorkers()[0]??await context.waitForEvent('serviceworker');
 const result=await worker.evaluate(async({url})=>{
  const response=await chrome.runtime.sendNativeMessage('org.rdm.download_manager',{version:1,id:'real-browser',command:{type:'add',spec:{url,queue_id:'main',idempotency_key:'brave-handoff',context:{cookies:[{name:'session',value:'approved',domain:'127.0.0.1',path:'/',secure:false,host_only:true}],referer:null,user_agent:navigator.userAgent}}}});return response;
 },{url:origin+'/auth'});
 if(!result.ok)throw new Error(result.error);
 let job;for(let i=0;i<100;i++){const state=JSON.parse(execFileSync(backend,['status'],{env,encoding:'utf8'}));job=state.downloads.find(j=>j.id===result.data.id);if(job?.status==='completed')break;await new Promise(r=>setTimeout(r,100));}
 if(job?.status!=='completed')throw new Error(JSON.stringify(job));
 if(!(await readFile(join(job.destination,job.filename))).equals(data))throw new Error('Browser download bytes differ');
 console.log('PASS real Brave authenticated native handoff downloads exact bytes');
 const download=page.getByRole('button',{name:'Download video',exact:true});
 await download.waitFor();await download.click();
 await page.getByRole('status').filter({hasText:'Added to RDM.'}).waitFor();
 const state=()=>JSON.parse(execFileSync(backend,['status'],{env,encoding:'utf8'}));
 for(let i=0;i<100&&!state().downloads.some(j=>j.url===origin+'/video.mp4'&&j.status==='completed');i++)await new Promise(r=>setTimeout(r,100));
 const video=state().downloads.find(j=>j.url===origin+'/video.mp4');
 if(video?.status!=='completed'||!(await readFile(join(video.destination,video.filename))).equals(data))throw new Error('Overlay file did not download exact bytes');
 await download.click();await page.getByRole('status').filter({hasText:'Added to RDM.'}).waitFor();
 if(state().downloads.filter(j=>j.url===origin+'/video.mp4').length!==1)throw new Error('Repeated overlay click was not deduplicated');
 await popup.bringToFront();await popup.locator('#overlay').uncheck();
 await page.getByRole('button',{name:'Download video',exact:true}).waitFor({state:'hidden'});
 await popup.locator('#overlay').check();await download.waitFor();
 await page.screenshot({path:'dist/screenshots/video-overlay-brave.png'});
 console.log('PASS real Brave video overlay -> native host -> Rust transfer, deduplication and overlay toggle');
 const blob=await context.newPage();await blob.goto(origin+'/blob-page');
 await blob.waitForFunction(()=>performance.getEntriesByType('resource').some(r=>r.name.endsWith('/stream/manifest.mpd')));
 await blob.getByRole('button',{name:'Download video',exact:true}).click();await blob.getByRole('status').filter({hasText:'Added to RDM.'}).waitFor();
 for(let i=0;i<200&&!state().downloads.some(j=>j.url===origin+'/stream/manifest.mpd'&&j.status==='completed');i++)await new Promise(r=>setTimeout(r,100));
 const manifestJob=state().downloads.find(j=>j.url===origin+'/stream/manifest.mpd');
 if(manifestJob?.status!=='completed')throw new Error('Blob player manifest was not downloaded: '+JSON.stringify(manifestJob));
 if(state().downloads.some(j=>j.url===origin+'/blob-page'))throw new Error('Unsupported player page was handed off instead of its observed manifest');
 const probe=JSON.parse(execFileSync('ffprobe',['-v','quiet','-show_streams','-of','json',join(manifestJob.destination,manifestJob.filename)],{encoding:'utf8'}));
 if(!['video','audio'].every(type=>probe.streams.some(s=>s.codec_type===type)))throw new Error('Captured manifest output is missing an audio/video track');
 console.log('PASS real blob-player resource capture -> native messaging -> Referer-protected DASH -> playable audio/video');

 const plain=await context.newPage();await plain.goto(origin+'/plain-player');
 await plain.waitForFunction(()=>performance.getEntriesByType('resource').some(r=>r.name.endsWith('/hls/index.txt')));
 await plain.getByRole('button',{name:'Download video',exact:true}).click();await plain.getByRole('status').filter({hasText:'Added to RDM.'}).waitFor();
 for(let i=0;i<200&&!state().downloads.some(j=>j.url===origin+'/stream/hls/playlist.txt'&&j.status==='completed');i++)await new Promise(r=>setTimeout(r,100));
 const plainJob=state().downloads.find(j=>j.url===origin+'/stream/hls/playlist.txt');
 if(plainJob?.status!=='completed')throw new Error('Plain-text HLS capture failed: '+JSON.stringify(plainJob));
 const plainProbe=JSON.parse(execFileSync('ffprobe',['-v','quiet','-show_streams','-of','json',join(plainJob.destination,plainJob.filename)],{encoding:'utf8'}));
 if(!['video','audio'].every(t=>plainProbe.streams.some(s=>s.codec_type===t)))throw new Error('Plain-text HLS output is missing audio/video');
 console.log('PASS actual Brave validates plain-text HLS master instead of sending the player page or an image-named segment');

 const outer=await context.newPage();await outer.goto(origin+'/plain-outer');
 await outer.frameLocator('iframe').getByRole('button',{name:'Download video',exact:true}).waitFor();
 await outer.bringToFront();await popup.reload();await popup.locator('#video').click();
 await popup.getByRole('status').filter({hasText:'Video added to RDM.'}).waitFor({timeout:10000}).catch(async()=>{throw new Error('Popup result: '+await popup.locator('#result').textContent());});
 if(state().downloads.filter(j=>j.url===origin+'/stream/hls/playlist.txt').length!==2||state().downloads.some(j=>j.url===origin+'/plain-outer'))throw new Error('Popup queued the outer page instead of the embedded stream');
 console.log('PASS actual extension popup resolves an embedded video frame and hands off its validated stream');

 // Serve a controlled page at the YouTube origin to verify page/SPA dispatch.
 // This checks extension integration; real YouTube extraction is a separate test.
 execFileSync(backend,['request',JSON.stringify({type:'upsert_queue',queue:{id:'main',name:'Main queue',concurrency:3,enabled:false,start_time:null,stop_time:null}})],{env});
 await context.route('https://www.youtube.com/**',route=>route.fulfill({contentType:'text/html',body:'<!doctype html><title>YouTube dispatch fixture</title><video width="720" height="405" controls></video>'}));
 const youtube=await context.newPage();await youtube.goto('https://www.youtube.com/watch?v=BaW_jenozKc&list=ignored');
 await youtube.getByRole('combobox',{name:'Video quality'}).selectOption('720');
 await youtube.getByRole('button',{name:'Download video',exact:true}).click();await youtube.getByRole('status').filter({hasText:'Added to RDM.'}).waitFor();
 const media=state().downloads.find(j=>j.url==='https://www.youtube.com/watch?v=BaW_jenozKc');
 if(media?.media?.max_height!==720||media.status!=='queued')throw new Error('YouTube overlay dispatched a segment or wrong quality');
 await youtube.evaluate(()=>history.pushState({},'', '/shorts/YE7VzlLtp-4'));
 await youtube.getByRole('button',{name:'Download video',exact:true}).click();
 await youtube.getByRole('status').filter({hasText:'Added to RDM.'}).waitFor();
 if(!state().downloads.some(j=>j.url==='https://www.youtube.com/watch?v=YE7VzlLtp-4'&&j.media?.max_height===720))throw new Error('SPA navigation retained the old video URL');
 console.log('PASS YouTube overlay quality selection, playlist exclusion and SPA navigation through real native messaging');
}finally{
 await context?.close();try{execFileSync(backend,['request','{"type":"shutdown"}'],{env,stdio:'ignore'});}catch{}
 await new Promise(r=>server.close(r));await rm(temp,{recursive:true,force:true});
}

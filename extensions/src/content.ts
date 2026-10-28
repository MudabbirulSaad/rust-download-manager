import browser from 'webextension-polyfill';
import {videoSource,resolveVideoSource,isMediaManifest,isManifestCandidate,type MediaResource} from './media-source';

// A single floating, shadow-isolated toolbar follows the visible video. Scans
// also cover SPA navigation, lazy players, open shadow roots and embedded frames.
const host=document.createElement('rdm-video-toolbar');
const shadow=host.attachShadow({mode:'open'});
shadow.innerHTML=`<style>
:host{all:initial;position:fixed;z-index:2147483647;display:none;color-scheme:light;font:13px/1.4 system-ui,sans-serif}
.bar{display:inline-flex;align-items:center;gap:1px;padding:3px;border:1px solid #64748b;border-radius:9px;background:#0f172a;color:#fff;box-shadow:0 4px 18px #0005}
button,select{font:inherit;color:inherit;background:transparent;border:0;border-radius:5px;min-height:38px;cursor:pointer}
button{display:flex;align-items:center;gap:7px;padding:8px 10px}select{max-width:94px;padding:5px}option{background:#0f172a;color:#fff}
button:hover,select:hover{background:#334155}button:focus-visible,select:focus-visible{outline:2px solid #93c5fd;outline-offset:1px}button:disabled{cursor:wait;opacity:.7}
svg{width:17px;height:17px;flex:none}.close{padding:8px}p{max-width:300px;box-sizing:border-box;margin:5px 0 0;padding:9px 12px;border-radius:7px;background:#0f172a;color:#fff;box-shadow:0 4px 18px #0005}p:empty{display:none}
</style><div class="bar" role="group" aria-label="Rust Download Manager video controls"><button id="download" type="button" title="Download this video with Rust Download Manager"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M12 3v12m-5-5 5 5 5-5M5 16v5h14v-5"/></svg><span>Download video</span></button><select aria-label="Video quality"><option value="0">Best</option><option value="2160">Up to 4K</option><option value="1440">1440p</option><option value="1080">1080p</option><option value="720">720p</option><option value="480">480p</option><option value="360">360p</option></select><button class="close" type="button" aria-label="Hide video download button for this page" title="Hide for this page">×</button></div><p role="status" aria-live="polite"></p>`;
const button=shadow.querySelector<HTMLButtonElement>('#download')!;
const select=shadow.querySelector<HTMLSelectElement>('select')!;
const status=shadow.querySelector<HTMLParagraphElement>('[role=status]')!;
let enabled=true, hiddenUrl='', active:HTMLVideoElement|null=null, busy=false;
let lastUrl=location.href, pendingKey:string|null=null, pendingSource='';
const mediaResources=new Map<string,MediaResource>();
let resourceEpoch=0;
function recordResources(entries:readonly PerformanceEntry[]){
  for(const entry of entries){
    const resource=entry as PerformanceResourceTiming&{contentType?:string};
    if(resource.startTime<resourceEpoch||!isMediaManifest(resource)&&!isManifestCandidate(resource))continue;
    mediaResources.delete(resource.name);mediaResources.set(resource.name,{name:resource.name,startTime:resource.startTime,contentType:resource.contentType});
    if(mediaResources.size>32)mediaResources.delete(mediaResources.keys().next().value!);
  }
}
recordResources(performance.getEntriesByType('resource'));
const resourceObserver=new PerformanceObserver(list=>recordResources(list.getEntries()));
resourceObserver.observe({type:'resource',buffered:true});
function changedPage(){
  if(location.href===lastUrl)return;
  lastUrl=location.href;status.textContent='';hiddenUrl='';pendingKey=null;pendingSource='';
  // Keep a short overlap for streams requested immediately after pushState.
  resourceEpoch=Math.max(0,performance.now()-3000);mediaResources.clear();
  recordResources(performance.getEntriesByType('resource'));
}
function sourceFor(video:HTMLVideoElement){
  changedPage();recordResources(performance.getEntriesByType('resource'));
  return videoSource(location.href,video.currentSrc||video.src||video.querySelector('source')?.src||'',Array.from(mediaResources.values()));
}

function videos(root:Document|ShadowRoot):HTMLVideoElement[] {
  const result=Array.from(root.querySelectorAll('video'));
  for(const element of root.querySelectorAll('*'))if(element.shadowRoot&&element!==host)result.push(...videos(element.shadowRoot));
  return result;
}
function update(){
  changedPage();
  if(!enabled||hiddenUrl===location.href){host.style.display='none';return;}
  const candidates=videos(document).filter(video=>{
    const r=video.getBoundingClientRect();return r.width>=220&&r.height>=120&&r.bottom>70&&r.top<innerHeight-60&&r.right>0&&r.left<innerWidth;
  }).sort((a,b)=>Number(b===document.activeElement)-Number(a===document.activeElement)||Number(!b.paused)-Number(!a.paused)||b.clientWidth*b.clientHeight-a.clientWidth*a.clientHeight);
  if(!busy)active=candidates[0]??null;
  if(!active||!active.isConnected){host.style.display='none';return;}
  const source=sourceFor(active);
  if(!source){host.style.display='none';return;}
  button.title=source.extract&&source.url===location.href
    ?'Resolve this player page with RDM. Start playback first to detect its stream.'
    :`Download detected video from ${new URL(source.url).hostname} with RDM`;
  const fullscreen=document.fullscreenElement;
  // A fullscreen video element itself cannot display DOM children. Retain the
  // popup action in that case; container-based player fullscreen is supported.
  if(fullscreen instanceof HTMLVideoElement){host.style.display='none';return;}
  const parent=fullscreen??document.documentElement;
  if(host.parentNode!==parent)parent.append(host);
  const r=active.getBoundingClientRect();
  host.style.display='block';
  host.style.top=`${Math.max(8,r.top+10)}px`;
  host.style.left=`${Math.max(8,Math.min(innerWidth-310,r.right-310))}px`;
  select.hidden=!source.extract;
}
// The popup asks the actual video frame, including embedded players, instead
// of sending the outer HTML page to the media provider.
browser.runtime.onMessage.addListener((message:unknown,sender:{id?:string})=>{
  if((message as {type?:string})?.type!=='rdm-probe-video'||sender.id!==browser.runtime.id)return undefined;
  update();const video=active;if(!video)return undefined;
  if(video.mediaKeys)return Promise.resolve({error:'This player uses protected media. RDM cannot download DRM video.'});
  const page=location.href;sourceFor(video);
  return resolveVideoSource(page,video.currentSrc||video.src||video.querySelector('source')?.src||'',Array.from(mediaResources.values()))
    .then(source=>{
      if(location.href!==page||!video.isConnected)return {error:'The video page changed. Try again.'};
      return source?{...source,page}:null;
    });
});
button.addEventListener('click',event=>{event.preventDefault();event.stopPropagation();if(busy||!active)return;
  // Handle a click immediately after SPA navigation, before the next scan.
  const video=active;sourceFor(video);
  if(active.mediaKeys){status.textContent='This player uses protected media. RDM cannot download DRM video.';return;}
  const page=location.href,quality=Number(select.value);
  busy=true;button.disabled=true;status.textContent='Checking video stream…';
  void resolveVideoSource(page,video.currentSrc||video.src||video.querySelector('source')?.src||'',Array.from(mediaResources.values()))
    .then(src=>{
      if(!src||location.href!==page)throw new Error('The video page changed. Try again.');
      const identity=`${src.url}|${quality}`;
      if(identity!==pendingSource||!pendingKey){pendingKey=crypto.randomUUID();pendingSource=identity;}
      status.textContent='Sending to RDM…';
      return browser.runtime.sendMessage({type:'rdm-video',url:src.url,page,extract:src.extract,maxHeight:quality,key:pendingKey});
    })
    .then(value=>{const response=value as {ok:boolean;error?:string};if(!response?.ok)throw new Error(response?.error??'Connection failed.');status.textContent='Added to RDM. You can close this page.';})
    .catch((error:unknown)=>{status.textContent=error instanceof Error?`${error.message} Test the connection in the RDM extension if needed.`:'Could not send this video. Test the connection in the RDM extension, then try again.';})
    .finally(()=>{busy=false;button.disabled=false;});
});
shadow.querySelector('.close')!.addEventListener('click',()=>{hiddenUrl=location.href;update();});
void browser.storage.local.get('videoOverlay').then(value=>{enabled=value.videoOverlay!==false;update();});
browser.storage.onChanged.addListener(changes=>{if(changes.videoOverlay){enabled=changes.videoOverlay.newValue!==false;update();}});
let scheduled=false;
function schedule(){if(scheduled)return;scheduled=true;requestAnimationFrame(()=>{scheduled=false;update();});}
const observer=new MutationObserver(schedule);
observer.observe(document.documentElement,{childList:true,subtree:true,attributes:true,attributeFilter:['src']});
addEventListener('scroll',schedule,true);addEventListener('resize',schedule);document.addEventListener('fullscreenchange',schedule);
document.addEventListener('play',schedule,true);document.addEventListener('loadedmetadata',schedule,true);
const timer=setInterval(schedule,1500);
addEventListener('pagehide',()=>{clearInterval(timer);observer.disconnect();resourceObserver.disconnect();},{once:true});
schedule();

export interface MediaResource {name:string;startTime:number;contentType?:string;verified?:boolean}

/** Only complete manifests are candidates. MSE segments are never whole videos. */
export function isMediaManifest(resource:Pick<MediaResource,'name'|'contentType'|'verified'>):boolean {
  try {
    const url=new URL(resource.name);
    return ['http:','https:'].includes(url.protocol)&&(
      resource.verified===true||/\.(?:m3u8|mpd)$/i.test(url.pathname)||
      /^(?:application\/(?:vnd\.apple\.mpegurl|x-mpegurl|dash\+xml)|audio\/mpegurl)(?:;|$)/i.test(resource.contentType??''));
  }catch{return false;}
}

/** A name is only a probe candidate; plain text is never assumed to be video. */
export function isManifestCandidate(resource:Pick<MediaResource,'name'>):boolean {
  try {
    const url=new URL(resource.name);
    return ['http:','https:'].includes(url.protocol)&&/(?:^|\/)(?:playlist|master|manifest|index)(?:[._-][\w-]+)?\.txt$/i.test(url.pathname);
  }catch{return false;}
}

function ranked(resources:readonly MediaResource[]){
  const newest=[...resources].sort((a,b)=>b.startTime-a.startTime);
  if(!newest.length)return newest;
  return newest.filter(r=>newest[0].startTime-r.startTime<30000)
    .sort((a,b)=>Number(/(?:^|\/)(?:master|manifest|playlist)(?:[._-]|$)/i.test(new URL(b.name).pathname))-Number(/(?:^|\/)(?:master|manifest|playlist)(?:[._-]|$)/i.test(new URL(a.name).pathname))||b.startTime-a.startTime);
}

function hls(text:string){
  return /^\uFEFF?#EXTM3U\r?\n/.test(text)&&/^#EXT-X-(?:STREAM-INF|TARGETDURATION|MEDIA-SEQUENCE|MEDIA):/m.test(text);
}

/** Probe only observed playlist-like URLs, after a user asks to download. */
export async function resolveVideoSource(pageUrl:string,source:string,resources:readonly MediaResource[],fetcher:typeof fetch=fetch){
  const selected=videoSource(pageUrl,source,resources);
  if(!selected||new URL(selected.url).hostname==='www.youtube.com'||selected.url!==pageUrl&&!isManifestCandidate({name:selected.url}))return selected;
  const candidates=[...resources];
  if(source&&isManifestCandidate({name:source}))candidates.push({name:source,startTime:Math.max(0,...resources.map(r=>r.startTime))+1});
  const probes=ranked(candidates.filter(isManifestCandidate)).slice(0,4);
  for(const candidate of probes){
    const controller=new AbortController();const timer=setTimeout(()=>controller.abort(),2500);
    try{
      // Content-script fetch follows the page's CORS and cookie rules. No new
      // host permissions or browser cookies are handed to the backend here.
      const response=await fetcher(candidate.name,{credentials:'same-origin',cache:'force-cache',signal:controller.signal});
      if(!response.ok||!response.body)continue;
      const reader=response.body.getReader(),decoder=new TextDecoder();let text='',size=0;
      try{
        while(size<65536){const {done,value}=await reader.read();if(done)break;const chunk=value.subarray(0,65536-size);size+=chunk.byteLength;text+=decoder.decode(chunk,{stream:true});if(hls(text)||size>=65536)break;}
      }finally{void reader.cancel().catch(()=>{});}
      if(hls(text))
        return {url:candidate.name,extract:true};
    }catch{ /* CORS, expired links and non-playlists retain page extraction. */ }
    finally{clearTimeout(timer);}
  }
  return selected;
}

/** Resolve the current video, never pass a blob or YouTube segment to Add URL. */
export function videoSource(pageUrl:string, source:string,resources:readonly MediaResource[]=[]):{url:string;extract:boolean}|null {
  let page:URL;
  try { page=new URL(pageUrl); } catch { return null; }
  if (!['http:','https:'].includes(page.protocol)) return null;
  const host=page.hostname.toLowerCase();
  const youtube=host==='youtu.be'||host==='youtube.com'||host.endsWith('.youtube.com')||host==='youtube-nocookie.com'||host.endsWith('.youtube-nocookie.com');
  if (youtube) {
    const id=host==='youtu.be'?page.pathname.slice(1).split('/')[0]:page.searchParams.get('v')??page.pathname.match(/^\/(?:shorts|embed|live)\/([^/]+)/)?.[1];
    if (id && /^[\w-]{11}$/.test(id)) return {url:`https://www.youtube.com/watch?v=${id}`,extract:true};
    return null;
  }
  try {
    const media=new URL(source,page);
    if (source&&['http:','https:'].includes(media.protocol))
      return {url:media.href,extract:isMediaManifest({name:media.href})||resources.some(r=>r.name===media.href&&isMediaManifest(r))};
  } catch { /* Page extraction can still handle MediaSource/blob players. */ }
  // Blob/MediaSource players fetch their manifest separately from video.src.
  // Resource timing exposes those URLs within this frame without extra host
  // permissions. Nothing is sent outside the browser until the user clicks.
  const manifests=ranked(resources.filter(isMediaManifest));
  if(manifests.length){
    return {url:manifests[0].name,extract:true};
  }
  return {url:page.href,extract:true};
}

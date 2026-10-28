import browser from 'webextension-polyfill';
import {native,handoff,originPattern,eligible} from './shared';
import {videoSource} from './media-source';
const result=document.querySelector<HTMLElement>('#result')!;
const checkbox=document.querySelector<HTMLInputElement>('#auto')!;
const show=(message:string)=>{result.textContent=message;};
const [tab]=await browser.tabs.query({active:true,currentWindow:true});
const overlay=document.querySelector<HTMLInputElement>('#overlay')!;
const stored=await browser.storage.local.get(['autoCapture','lastResult','videoOverlay']);checkbox.checked=Boolean(stored.autoCapture);overlay.checked=stored.videoOverlay!==false;
if(typeof stored.lastResult==='string')show(stored.lastResult);
document.querySelector('#test')!.addEventListener('click',()=>{void native({type:'health'}).then(()=>{show('Connected to the Rust background service.');return browser.action.setBadgeText({text:''});}).catch(()=>show('Connection failed. Register the browser connection in the desktop app, then reload this extension.'));});
document.querySelector('#send')!.addEventListener('click',()=>{if(!tab?.url||!eligible(tab.url)){show('Open an HTTP or HTTPS page first.');return;}void handoff(tab.url,crypto.randomUUID(),undefined,tab.cookieStoreId).then(()=>show('URL added to RDM.')).catch(()=>show('Handoff failed. The current page was left unchanged.'));});
document.querySelector('#approve')!.addEventListener('click',()=>{
 if(!tab?.url||!eligible(tab.url)){show('Open the site you want to approve first.');return;}
 const origin=new URL(tab.url).origin;
 void browser.permissions.request({permissions:['cookies'],origins:[originPattern(tab.url)]}).then(async granted=>{
  if(granted){const {approvedSites}=await browser.storage.local.get('approvedSites');await browser.storage.local.set({approvedSites:[...new Set([...(Array.isArray(approvedSites)?approvedSites:[]),origin])]});}
  show(granted?'This site is approved. Its cookies can accompany downloads sent to RDM.':'Site access was not granted. Public downloads still work.');
 }).catch(()=>show('Could not request site access.'));
});
checkbox.addEventListener('change',()=>{void browser.storage.local.set({autoCapture:checkbox.checked});show(checkbox.checked?'Automatic capture enabled for approved sites and known GET requests.':'Automatic capture disabled.');});
overlay.addEventListener('change',()=>{void browser.storage.local.set({videoOverlay:overlay.checked});show(overlay.checked?'Video download buttons enabled. Reload pages opened before the extension.':'Video download buttons hidden.');});
const videoButton=document.querySelector<HTMLButtonElement>('#video')!;
videoButton.addEventListener('click',()=>{void(async()=>{
 if(tab?.id===undefined){show('Open a video page first.');return;}
 const quality=Number(document.querySelector<HTMLSelectElement>('#quality')!.value);
 videoButton.disabled=true;show('Checking the video player…');
 try{
  let source=await browser.tabs.sendMessage(tab.id,{type:'rdm-probe-video'}).catch(()=>null) as {url:string;extract:boolean;page:string;error?:string}|null;
  if(source?.error)throw new Error(source.error);
  if(!source){
   const page=tab.url?videoSource(tab.url,''):null;
   if(page&&new URL(page.url).hostname==='www.youtube.com')source={...page,page:tab.url!};
   else throw new Error('No video player was detected. Refresh the video tab, start playback, then use its on-video Download video button.');
  }
  await handoff(source.url,crypto.randomUUID(),source.page,tab.cookieStoreId,source.extract?{max_height:quality}:null);
  show('Video added to RDM. Follow its progress in the desktop app.');
 }catch(error){show(error instanceof Error?error.message:'Video handoff failed. Test the connection, then try again.');}
 finally{videoButton.disabled=false;}
})();});

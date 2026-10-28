import browser from 'webextension-polyfill';
import {eligible,handoff,originPattern,siteApproved} from './shared';
import {videoSource} from './media-source';
const MENU='rdm-download';
const requests=new Map<string,{method:string;at:number;tabId:number}>();
browser.runtime.onInstalled.addListener(async()=>{
  await browser.contextMenus.removeAll();
  browser.contextMenus.create({id:MENU,title:'Download with Rust Download Manager',contexts:['link']});
  const value=await browser.storage.local.get('autoCapture');
  if(value.autoCapture===undefined)await browser.storage.local.set({autoCapture:false});
});
// Only our extension's content scripts may enqueue media, and each target is
// checked against the actual sending frame. Website postMessage is never used.
browser.runtime.onMessage.addListener((message:unknown,sender:{tab?:{cookieStoreId?:string};url?:string})=>{
  const m=message as {type?:string;url?:string;page?:string;extract?:boolean;maxHeight?:number;key?:string};
  if(m?.type!=='rdm-video')return undefined;
  if(!sender.tab||!sender.url||!eligible(sender.url)||!m.url||!eligible(m.url)||!m.key||m.key.length>200||![0,360,480,720,1080,1440,2160].includes(m.maxHeight??-1))return Promise.resolve({ok:false,error:'Invalid video request.'});
  // Chromium's sender.url can retain the original URL after history.pushState.
  // The frame's origin remains authoritative across same-document navigation.
  if(!m.page||!eligible(m.page)||new URL(m.page).origin!==new URL(sender.url).origin)return Promise.resolve({ok:false,error:'The video page changed. Try again.'});
  const normalized=videoSource(m.page,m.extract?'':m.url);
  const url=normalized?.extract&&/youtube\.com/.test(new URL(normalized.url).hostname)?normalized.url:m.url;
  return handoff(url,m.key,m.page,sender.tab.cookieStoreId,m.extract?{max_height:m.maxHeight!}:null)
    .then(async()=>{await browser.storage.local.set({lastResult:'Video added to RDM.'});return {ok:true};})
    .catch((error:unknown)=>({ok:false,error:error instanceof Error?error.message:'Video handoff failed. Test the native connection.'}));
});
browser.contextMenus.onClicked.addListener((info,tab)=>{
  if(info.menuItemId!==MENU||!info.linkUrl)return;
  void handoff(info.linkUrl,crypto.randomUUID(),info.pageUrl,tab?.cookieStoreId).then(()=>browser.storage.local.set({lastResult:'Download added to RDM.'})).catch(async()=>{
    await browser.storage.local.set({lastResult:'Handoff failed. Check the native connection and try again; the browser link is unchanged.'});
    await browser.action.setBadgeText({text:'!'});
  });
});
browser.webRequest.onBeforeRequest.addListener(details=>{
  requests.set(details.url,{method:details.method,at:Date.now(),tabId:details.tabId});
  if(requests.size>500)for(const[key,value]of requests)if(Date.now()-value.at>30000)requests.delete(key);
},{urls:['http://*/*','https://*/*']});
browser.downloads.onCreated.addListener(item=>{void(async()=>{
  const {autoCapture}=await browser.storage.local.get('autoCapture');
  if(!autoCapture||!eligible(item.url)||item.incognito||item.state!=='in_progress')return;
  if(!await siteApproved(item.url))return;
  // Only capture requests whose method is known. POST and browser-only transfers stay in the browser.
  const observation=requests.get(item.url);
  if(!observation||observation.method!=='GET'||Date.now()-observation.at>30000)return;
  if(!await browser.permissions.contains({origins:[originPattern(item.url)]}))return;
  try{
    await browser.downloads.pause(item.id);
    const tab=observation.tabId>=0?await browser.tabs.get(observation.tabId).catch(()=>undefined):undefined;
    await handoff(item.url,`browser-${item.id}-${item.startTime}`,item.referrer,tab?.cookieStoreId);
  }catch{
    await browser.downloads.resume(item.id).catch(()=>{});
    await browser.storage.local.set({lastResult:'RDM was unavailable. The browser download was retained.'});return;
  }
  await browser.downloads.cancel(item.id).catch(async()=>{
    await browser.storage.local.set({lastResult:'RDM accepted the file, but the browser could not cancel its own copy.'});
  });
})().catch(()=>{});});

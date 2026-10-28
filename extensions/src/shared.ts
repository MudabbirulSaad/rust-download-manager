import browser from 'webextension-polyfill';
import type { AddSpec, RequestContext, ScopedCookie, MediaOptions } from '../../src/protocol';
export const HOST='org.rdm.download_manager';
export function eligible(url:string){try{return ['http:','https:'].includes(new URL(url).protocol);}catch{return false;}}
export function originPattern(url:string){return `${new URL(url).protocol}//${new URL(url).hostname}/*`;}
export async function siteApproved(url:string){
  const {approvedSites}=await browser.storage.local.get('approvedSites');
  return Array.isArray(approvedSites)&&approvedSites.includes(new URL(url).origin);
}
export async function native(command:unknown){
  const response=await browser.runtime.sendNativeMessage(HOST,{version:1,id:crypto.randomUUID(),command}) as {ok:boolean;data:unknown;error?:string};
  if(!response.ok)throw new Error(response.error??'Native host rejected the request');return response.data;
}
export function requestReferer(target:string,page?:string):string|null {
  if(!page)return null;
  try{
    const from=new URL(page),to=new URL(target);
    if(!eligible(from.href)||!eligible(to.href)||from.username||from.password||from.protocol==='https:'&&to.protocol==='http:')return null;
    from.hash='';return from.origin===to.origin?from.href:`${from.origin}/`;
  }catch{return null;}
}
export async function contextFor(url:string,referer?:string,storeId?:string):Promise<RequestContext|null>{
  const pattern=originPattern(url);
  const context:RequestContext={cookies:[],referer:requestReferer(url,referer),user_agent:navigator.userAgent};
  // DOM access for overlays must never become cookie approval for every site.
  if(!await siteApproved(url))return context;
  if(!await browser.permissions.contains({permissions:['cookies'],origins:[pattern]}))return context;
  const cookies=await browser.cookies.getAll({url,...(storeId?{storeId}:{})});
  const scoped:ScopedCookie[]=cookies.map(c=>({name:c.name,value:c.value,domain:c.domain,path:c.path,secure:c.secure,host_only:c.hostOnly}));
  return {...context,cookies:scoped};
}
export async function handoff(url:string,key:string,referer?:string,storeId?:string,media:MediaOptions|null=null){
  if(!eligible(url))throw new Error('Only HTTP and HTTPS links can be handed off.');
  const spec:AddSpec={url,destination:null,filename:null,queue_id:'main',idempotency_key:key,context:await contextFor(url,referer,storeId),media};
  if(media){
    const health=await native({type:'health'}) as {capabilities?:string[]};
    if(!health.capabilities?.includes('media-v1'))throw new Error('Restart the background service after updating RDM to enable video downloads.');
  }
  return native({type:media?'add_media':'add',spec});
}

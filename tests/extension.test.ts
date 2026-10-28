import{beforeEach,describe,it,expect,vi}from'vitest';
const mock=vi.hoisted(()=>({contains:vi.fn(),getAll:vi.fn(),native:vi.fn(),stored:vi.fn()}));
vi.mock('webextension-polyfill',()=>({default:{permissions:{contains:mock.contains},cookies:{getAll:mock.getAll},runtime:{sendNativeMessage:mock.native},storage:{local:{get:mock.stored}}}}));
import{eligible,originPattern,contextFor,handoff,requestReferer}from'../extensions/src/shared';
beforeEach(()=>{vi.clearAllMocks();mock.stored.mockResolvedValue({approvedSites:['https://example.com']});});
describe('extension handoff boundary',()=>{
 it('restricts links to HTTP and HTTPS',()=>{expect(eligible('blob:https://example.com/a')).toBe(false);expect(eligible('file:///etc/passwd')).toBe(false);expect(eligible('https://example.com/a')).toBe(true);});
 it('uses browser match patterns without unsupported port syntax',()=>{expect(originPattern('http://localhost:8000/file')).toBe('http://localhost/*');});
 it('sends public request context without reading cookies before approval',async()=>{mock.contains.mockResolvedValue(false);expect(await contextFor('https://example.com/a','https://example.com/player')).toMatchObject({cookies:[],referer:'https://example.com/player'});expect(mock.getAll).not.toHaveBeenCalled();});
 it('does not treat broad overlay access or another approved site as cookie approval',async()=>{mock.contains.mockResolvedValue(true);expect(await contextFor('https://unapproved.example/a')).toMatchObject({cookies:[]});expect(mock.getAll).not.toHaveBeenCalled();});
 it('strips query credentials across origins and drops downgrade referrers',()=>{
  expect(requestReferer('https://cdn.example/master.m3u8','https://player.example/e/id?token=secret#part')).toBe('https://player.example/');
  expect(requestReferer('http://cdn.example/file','https://player.example/watch')).toBeNull();
  expect(requestReferer('https://cdn.example/file','https://user:secret@player.example/watch')).toBeNull();
 });
 it('uses the exact URL and cookie store after permission',async()=>{mock.contains.mockResolvedValue(true);mock.getAll.mockResolvedValue([{name:'session',value:'test',domain:'example.com',path:'/',secure:true,hostOnly:true}]);const context=await contextFor('https://example.com/a','https://example.com/page','container-2');expect(mock.getAll).toHaveBeenCalledWith({url:'https://example.com/a',storeId:'container-2'});expect(context?.cookies[0]).toMatchObject({domain:'example.com',host_only:true});});
 it('treats a negative native acknowledgement as a failed handoff',async()=>{mock.contains.mockResolvedValue(false);mock.native.mockResolvedValue({ok:false,error:'Service unavailable'});await expect(handoff('https://example.com/file','test-key')).rejects.toThrow('Service unavailable');});
 it('durably hands a video page and quality selection to the native service',async()=>{
  mock.contains.mockResolvedValue(false);mock.native.mockResolvedValueOnce({ok:true,data:{capabilities:['media-v1']}}).mockResolvedValue({ok:true,data:{id:'video'}});
  await handoff('https://www.youtube.com/watch?v=BaW_jenozKc','video-key',undefined,undefined,{max_height:1080});
  expect(mock.native).toHaveBeenCalledWith('org.rdm.download_manager',expect.objectContaining({command:{type:'add_media',spec:expect.objectContaining({media:{max_height:1080},idempotency_key:'video-key'})}}));
 });
 it('does not send media as an HTML download to an older service',async()=>{
  mock.contains.mockResolvedValue(false);mock.native.mockResolvedValue({ok:true,data:{version:1}});
  await expect(handoff('https://www.youtube.com/watch?v=BaW_jenozKc','key',undefined,undefined,{max_height:720})).rejects.toThrow('Restart');
  expect(mock.native).toHaveBeenCalledTimes(1);
 });
});

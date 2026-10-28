import {afterEach,describe,it,expect,vi}from'vitest';
import{cleanup,render,screen,waitFor}from'@testing-library/react';
import userEvent from '@testing-library/user-event';
import App from '../src/App';
import{bytes,duration}from'../src/api';
const mock=vi.hoisted(()=>({command:vi.fn(async()=>({id:'new'})),snapshot:{downloads:[],queues:[{id:'main',name:'Main queue',concurrency:3,enabled:true,start_time:null,stop_time:null}],settings:{max_downloads:3,connections_per_file:4,connections_per_origin:8,speed_limit:0,download_directory:'/tmp/downloads'}}}));
vi.mock('@tauri-apps/api/core',()=>({isTauri:()=>true,invoke:vi.fn(async(name,args)=>name==='connect_service'?mock.snapshot:name==='service_request'?mock.command(args.command):undefined)}));
vi.mock('@tauri-apps/api/event',()=>({listen:vi.fn(async()=>()=>{})}));
vi.mock('@tauri-apps/plugin-dialog',()=>({open:vi.fn(async()=>null)}));
afterEach(()=>{cleanup();mock.command.mockClear();mock.snapshot.downloads.length=0;});
describe('download manager',()=>{
 it('displays an honest empty library and connection',async()=>{render(<App/>);expect(await screen.findByText('Service connected')).toBeInTheDocument();expect(screen.getByRole('heading',{name:'Your next download starts here'})).toBeInTheDocument();});
 it('adds a batch through the typed service interface',async()=>{const user=userEvent.setup();render(<App/>);await screen.findByText('Service connected');await user.click(screen.getByRole('button',{name:'Add download'}));await user.type(screen.getByLabelText('Download URLs'),'https://example.com/a.zip\nhttps://example.com/b.zip');await user.click(screen.getByRole('button',{name:'Start downloads'}));await waitFor(()=>expect(mock.command).toHaveBeenCalledTimes(2));expect(mock.command.mock.calls[0][0]).toMatchObject({type:'add',spec:{url:'https://example.com/a.zip',destination:'/tmp/downloads'}});});
 it('saves a queue schedule and concurrency',async()=>{const user=userEvent.setup();render(<App/>);await screen.findByText('Service connected');await user.click(screen.getByRole('button',{name:'Scheduler'}));await user.click(screen.getByRole('button',{name:'New queue'}));await user.clear(screen.getByLabelText('Queue name'));await user.type(screen.getByLabelText('Queue name'),'Overnight');await user.click(screen.getByRole('button',{name:'Save queue'}));await waitFor(()=>expect(mock.command).toHaveBeenCalledWith(expect.objectContaining({type:'upsert_queue',queue:expect.objectContaining({name:'Overnight'})})));});
 it('shows browser setup instructions',async()=>{const user=userEvent.setup();render(<App/>);await user.click(screen.getByRole('button',{name:/Connect your browser/}));expect(screen.getByRole('button',{name:'Register browser connection'})).toBeInTheDocument();});
 it('formats sizes and remaining time',()=>{expect(bytes(1024)).toBe('1.0 KiB');expect(bytes(null)).toBe('Unknown');expect(duration(65)).toBe('1m 5s');});
 it('sends YouTube pages as media jobs with the chosen quality',async()=>{
  const user=userEvent.setup();render(<App/>);await screen.findByText('Service connected');await user.click(screen.getByRole('button',{name:'Add download'}));
  await user.type(screen.getByLabelText('Download URLs'),'https://www.youtube.com/watch?v=BaW_jenozKc');
  await user.selectOptions(screen.getByLabelText('Video quality'),'720');await user.click(screen.getByRole('button',{name:'Start downloads'}));
  await waitFor(()=>expect(mock.command).toHaveBeenCalledWith(expect.objectContaining({type:'add_media',spec:expect.objectContaining({media:{max_height:720}})})));
 });
 it('shows a failed download error in its row and opens recovery details',async()=>{
  const error='This player page is unsupported. Play the video in your browser and use its on-video button.';
  mock.snapshot.downloads.push({id:'failed',url:'https://player.example/e/test',filename:'Video (resolving).mkv',destination:'/tmp/downloads',queue_id:'main',status:'needs_attention',total:null,downloaded:0,speed:0,active_connections:0,completed_ranges:[],validator:null,resumable:true,authenticated:false,error,created_at:'2026-10-28T05:30:00Z',speed_limit:0,idempotency_key:null,final_url:null,sha256:null,media:{max_height:0},media_stage:'extracting'} as never);
  const user=userEvent.setup();render(<App/>);
  const button=await screen.findByRole('button',{name:'View error for Video (resolving).mkv'});
  expect(button).toHaveTextContent('This player page is unsupported');await user.click(button);
  expect(await screen.findByRole('alert')).toHaveTextContent(error);
 });
});

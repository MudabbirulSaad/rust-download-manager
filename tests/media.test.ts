import {describe,it,expect,vi} from 'vitest';
import {videoSource,resolveVideoSource,isMediaManifest,isManifestCandidate} from '../extensions/src/media-source';

describe('video source selection',()=>{
  it('uses a YouTube watch page instead of its blob or CDN segments',()=>{
    expect(videoSource('https://www.youtube.com/watch?v=BaW_jenozKc&list=123','blob:https://www.youtube.com/id')).toEqual({url:'https://www.youtube.com/watch?v=BaW_jenozKc',extract:true});
    expect(videoSource('https://www.youtube.com/shorts/BaW_jenozKc','https://rr1.googlevideo.com/videoplayback')).toEqual({url:'https://www.youtube.com/watch?v=BaW_jenozKc',extract:true});
    expect(videoSource('https://www.youtube-nocookie.com/embed/BaW_jenozKc','')).toEqual({url:'https://www.youtube.com/watch?v=BaW_jenozKc',extract:true});
  });
  it('never creates a YouTube playlist or home-page download',()=>{
    expect(videoSource('https://www.youtube.com/','')).toBeNull();
    expect(videoSource('https://www.youtube.com/playlist?list=123','')).toBeNull();
  });
  it('uses the Rust HTTP engine for direct videos, preserving signed queries',()=>{
    expect(videoSource('https://example.com/page','https://cdn.example.com/movie.mp4?signature=test')).toEqual({url:'https://cdn.example.com/movie.mp4?signature=test',extract:false});
    expect(videoSource('https://example.com/page','/movie.webm')).toEqual({url:'https://example.com/movie.webm',extract:false});
  });
  it('uses the media provider for HLS, DASH and blob players',()=>{
    expect(videoSource('https://example.com/watch','/stream.m3u8?token=test')?.extract).toBe(true);
    expect(videoSource('https://example.com/watch','https://cdn.example.com/manifest.mpd')?.extract).toBe(true);
    expect(videoSource('https://example.com/watch','blob:https://example.com/123')).toEqual({url:'https://example.com/watch',extract:true});
  });
  it('rejects privileged pages and lookalike YouTube domains',()=>{
    expect(videoSource('file:///tmp/video.html','')).toBeNull();
    expect(videoSource('https://youtube.com.attacker.test/page','/test.mp4')?.extract).toBe(false);
  });
  it('uses the browser-observed manifest for an unsupported blob player',()=>{
    expect(videoSource('https://player.example/e/test','blob:https://player.example/id',[
      {name:'https://cdn.example/master.m3u8?token=test',startTime:100},
      {name:'https://cdn.example/720p/index.m3u8?token=test',startTime:110},
      {name:'https://cdn.example/720p/segment-001.ts',startTime:120},
    ])).toEqual({url:'https://cdn.example/master.m3u8?token=test',extract:true});
  });
  it('never treats an MSE segment or an image query as a full video manifest',()=>{
    expect(isMediaManifest({name:'https://cdn.example/segment.mp4'})).toBe(false);
    expect(isMediaManifest({name:'https://cdn.example/poster.jpg?next=master.m3u8'})).toBe(false);
    expect(videoSource('https://player.example/e/test','blob:https://player.example/id',[{name:'https://cdn.example/segment.mp4',startTime:10}])).toEqual({url:'https://player.example/e/test',extract:true});
  });
  it('recognizes extensionless manifests by their observed content type',()=>{
    expect(videoSource('https://player.example/e/test','blob:https://player.example/id',[{name:'https://cdn.example/play?id=test',startTime:10,contentType:'application/vnd.apple.mpegurl'}])).toEqual({url:'https://cdn.example/play?id=test',extract:true});
  });
  it('keeps YouTube extraction on the page even when a DASH resource was observed',()=>{
    expect(videoSource('https://www.youtube.com/watch?v=BaW_jenozKc','blob:https://www.youtube.com/id',[{name:'https://cdn.example/manifest.mpd',startTime:10}])?.url).toBe('https://www.youtube.com/watch?v=BaW_jenozKc');
  });
  it('validates disguised HLS playlists by content, preserving their signed URL',async()=>{
    const master='https://cdn.example/u/signed/playlist.txt?token=test';
    const fetcher=vi.fn().mockResolvedValue(new Response('#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000000\n720p/index.txt\n'));
    const resources=[{name:master,startTime:100},{name:'https://cdn.example/u/signed/720p/index.txt',startTime:110},{name:'https://cdn.example/u/signed/720p/seg_00000.png',startTime:120}];
    expect(isMediaManifest(resources[0])).toBe(false);
    expect(await resolveVideoSource('https://player.example/e/id','blob:https://player.example/id',resources,fetcher)).toEqual({url:master,extract:true});
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(fetcher).toHaveBeenCalledWith(master,expect.objectContaining({credentials:'same-origin'}));
  });
  it('never hands ordinary text, HTML or disguised image segments to the media worker',async()=>{
    const fetcher=vi.fn().mockResolvedValue(new Response('<html>#EXTM3U\n#EXT-X-STREAM-INF:100\n</html>'));
    expect(await resolveVideoSource('https://player.example/watch','blob:https://player.example/id',[{name:'https://cdn.example/playlist.txt',startTime:1}],fetcher)).toEqual({url:'https://player.example/watch',extract:true});
    expect(isManifestCandidate({name:'https://cdn.example/seg_00000.png'})).toBe(false);
    expect(isManifestCandidate({name:'https://cdn.example/notes.txt?file=playlist.txt'})).toBe(false);
  });
  it('retains direct media and canonical YouTube dispatch without probing unrelated resources',async()=>{
    const fetcher=vi.fn(),resources=[{name:'https://cdn.example/playlist.txt',startTime:1}];
    expect(await resolveVideoSource('https://player.example/watch','https://cdn.example/movie.mp4',resources,fetcher)).toEqual({url:'https://cdn.example/movie.mp4',extract:false});
    expect(await resolveVideoSource('https://www.youtube.com/watch?v=BaW_jenozKc','blob:https://www.youtube.com/id',resources,fetcher)).toEqual({url:'https://www.youtube.com/watch?v=BaW_jenozKc',extract:true});
    expect(fetcher).not.toHaveBeenCalled();
  });
  it('keeps unsupported page extraction when the browser denies a playlist probe',async()=>{
    const fetcher=vi.fn().mockRejectedValue(new TypeError('CORS blocked'));
    expect(await resolveVideoSource('https://player.example/watch','',[{name:'https://cdn.example/playlist.txt',startTime:1}],fetcher)).toEqual({url:'https://player.example/watch',extract:true});
  });
});

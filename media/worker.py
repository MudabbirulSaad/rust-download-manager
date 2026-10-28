#!/usr/bin/env python3
"""Private JSON-lines worker. Rust owns its process group, state and final rename.

Cookies arrive on stdin and stay in memory. No browser database, user yt-dlp
configuration, plugins, shell command or credential file is read or written.
"""
import ctypes
import http.cookiejar
import json
import os
from pathlib import Path
import signal
import sys
import time
from urllib.parse import urlsplit, urlunsplit


def emit(**value):
    print(json.dumps(value, ensure_ascii=True), flush=True)


def parent_died(_sig, _frame):
    os.killpg(os.getpgrp(), signal.SIGKILL)


signal.signal(signal.SIGTERM, parent_died)
parent = os.getppid()
# Also terminate FFmpeg and Node if the Rust daemon is forcibly terminated.
ctypes.CDLL(None).prctl(1, signal.SIGTERM, 0, 0, 0)
if parent == 1 or os.getppid() != parent:
    parent_died(None, None)
os.umask(0o077)
request = json.load(sys.stdin)
sys.path.insert(0, request['yt_dlp'])
import yt_dlp  # noqa: E402 -- pinned zipimport distribution


class QuietLogger:
    def debug(self, _message): pass
    def info(self, _message): pass
    def warning(self, _message): pass
    def error(self, _message): pass


last_update = 0.0
streams = {}


def progress(value):
    global last_update
    key = value.get('filename', 'media')
    streams[key] = (value.get('downloaded_bytes') or 0,
                    value.get('total_bytes') or value.get('total_bytes_estimate') or 0)
    if time.monotonic() - last_update < 0.25 and value['status'] != 'finished':
        return
    last_update = time.monotonic()
    info = value.get('info_dict', {})
    emit(event='progress', downloaded=sum(v[0] for v in streams.values()),
         total=sum(v[1] for v in streams.values()) or None,
         speed=int(value.get('speed') or 0), title=info.get('title', 'Video'))


def postprocess(value):
    emit(event='stage', stage='merging')


def filter_video(info, *, incomplete):
    if info.get('is_live') or info.get('live_status') in ('is_live', 'is_upcoming'):
        return 'Live broadcasts are not supported; try again after the broadcast ends.'
    return None


height = request['max_height']
cap = f'[height<=?{height}]' if height else ''
options = {
    'format': f'bv*{cap}+ba/b{cap}',
    'outtmpl': str(Path(request['directory']) / 'media.%(ext)s'),
    'noplaylist': True, 'playlist_items': '1', 'match_filter': filter_video,
    'quiet': True, 'noprogress': True, 'logger': QuietLogger(),
    'cachedir': False, 'concurrent_fragment_downloads': 1,
    'continuedl': True, 'overwrites': False, 'retries': 5, 'fragment_retries': 5,
    'socket_timeout': 20, 'restrictfilenames': True,
    'merge_output_format': 'mkv',
    'progress_hooks': [progress], 'postprocessor_hooks': [postprocess],
    'js_runtimes': {'node': {'path': request['node']}},
    'remote_components': set(), 'plugin_dirs': [],
    'ratelimit': request['rate_limit'] or None,
}
context = request.get('context') or {}
headers = {}
if context.get('user_agent'):
    headers['User-Agent'] = context['user_agent']
if context.get('referer'):
    referer = urlsplit(context['referer'])
    target = urlsplit(request['url'])
    if (referer.scheme in ('http', 'https') and referer.hostname
            and not referer.username and not referer.password
            and not (referer.scheme == 'https' and target.scheme == 'http')):
        # Provider extractors can contact other origins. An origin-only Referer
        # supports embedded/CDN hotlink checks without forwarding page tokens.
        headers['Referer'] = urlunsplit((referer.scheme, referer.netloc, '/', '', ''))
        if (referer.scheme, referer.netloc) != (target.scheme, target.netloc):
            # HLS fetches from a separate CDN commonly require the same public
            # Origin header as the browser's CORS request.
            headers['Origin'] = urlunsplit((referer.scheme, referer.netloc, '', '', ''))
if headers:
    options['http_headers'] = headers
# CookieJar applies domain/path/secure rules to every request; no Cookie or
# Authorization header is attached globally.
try:
    with yt_dlp.YoutubeDL(options) as downloader:
        downloader.cookiejar.set_policy(http.cookiejar.DefaultCookiePolicy(
            strict_ns_domain=http.cookiejar.DefaultCookiePolicy.DomainStrictNonDomain))
        for cookie in context.get('cookies', []):
            downloader.cookiejar.set_cookie(http.cookiejar.Cookie(
                version=0, name=cookie['name'], value=cookie['value'],
                port=None, port_specified=False, domain=cookie['domain'],
                domain_specified=not cookie['host_only'],
                domain_initial_dot=cookie['domain'].startswith('.'),
                path=cookie['path'] or '/', path_specified=True, secure=cookie['secure'],
                expires=None, discard=True, comment=None, comment_url=None, rest={}))
        info = downloader.extract_info(request['url'], download=True)
        if not info or info.get('_type') in ('playlist', 'multi_video'):
            raise RuntimeError('No single video was found on this page.')
        candidates = [p for p in Path(request['directory']).glob('media.*')
                      if p.suffix.lower() in ('.mp4', '.mkv', '.webm', '.mov', '.flv', '.ogg', '.ogv', '.m4v', '.ts')]
        # Format intermediates have an extra dot, such as media.f137.mp4.
        candidates = [p for p in candidates if p.name.count('.') == 1]
        if len(candidates) != 1:
            raise RuntimeError('The video did not produce one complete output file.')
        emit(event='complete', path=candidates[0].name, title=info.get('title', 'Video'))
except Exception as error:
    text = str(error).lower()
    if 'unsupported url' in text or 'no video formats found' in text:
        message = 'This player page is unsupported. Play the video in your browser, then click its on-video Download video button to capture the actual stream. If it still fails, this player needs browser-bound support.'
    elif any(word in text for word in ['drm', 'protected']):
        message = 'This video is DRM protected and cannot be downloaded by RDM.'
    elif any(word in text for word in ['sign in', 'login', 'cookies', '403', '401', 'bot']):
        message = 'The video site requires fresh authorization or blocked this request. Approve this site in the extension and send the video again.'
    elif any(word in text for word in ['space', 'permission denied', 'read-only']):
        message = 'Cannot write the video. Check free disk space and destination permissions, then resume.'
    elif 'ffmpeg' in text:
        message = 'FFmpeg could not process this video. Check that the ffmpeg package is installed and resume.'
    elif 'live' in text:
        message = 'Live broadcasts are not supported. Send the video after the broadcast ends.'
    else:
        message = 'This page could not be downloaded. It may be unsupported, unavailable, or need a newer RDM media provider. Open the video page and try again.'
    emit(event='error', message=message)
    sys.exit(1)

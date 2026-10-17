// Network first for everything, so a new version is used as soon as it is
// served; the copy kept of every page, picture and list already opened is
// what answers when there is no coverage.
'use strict';
const CACHE = 'openmanuals-v2';
const SHELL = ['/', '/app.js', '/app.css', '/reader.js', '/reader.css', '/icon.svg', '/manifest.webmanifest'];

self.addEventListener('install', e => e.waitUntil(caches.open(CACHE).then(c => c.addAll(SHELL)).then(() => self.skipWaiting())));
self.addEventListener('activate', e => e.waitUntil(
  caches.keys().then(keys => Promise.all(keys.filter(k => k !== CACHE).map(k => caches.delete(k)))).then(() => self.clients.claim())));

self.addEventListener('fetch', e => {
  const u = new URL(e.request.url);
  if (e.request.method !== 'GET' || u.origin !== location.origin) return;
  e.respondWith(fetch(e.request).then(r => {
    if (r.ok) { const c = r.clone(); caches.open(CACHE).then(x => x.put(e.request, c)); }
    return r;
  }).catch(() => caches.match(e.request).then(r => r || (u.pathname.startsWith('/api/') ? Response.error() : caches.match('/')))));
});

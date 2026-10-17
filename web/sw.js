// Keeps the app shell and every document, picture and list already opened, so
// what was read once still opens without coverage.
'use strict';
const SHELL = 'shell-v1';
const DATA = 'data-v1';
const FILES = ['/', '/app.js', '/app.css', '/reader.js', '/reader.css', '/icon.svg', '/manifest.webmanifest'];

self.addEventListener('install', e => e.waitUntil(caches.open(SHELL).then(c => c.addAll(FILES)).then(() => self.skipWaiting())));
self.addEventListener('activate', e => e.waitUntil(self.clients.claim()));

self.addEventListener('fetch', e => {
  const u = new URL(e.request.url);
  if (e.request.method !== 'GET' || u.origin !== location.origin) return;
  if (u.pathname.startsWith('/api/')) {
    // network first, the copy when offline
    e.respondWith(fetch(e.request).then(r => {
      if (r.ok) { const c = r.clone(); caches.open(DATA).then(x => x.put(e.request, c)); }
      return r;
    }).catch(() => caches.match(e.request).then(r => r || Response.error())));
    return;
  }
  e.respondWith(caches.match(e.request).then(r => r || fetch(e.request).catch(() => caches.match('/'))));
});

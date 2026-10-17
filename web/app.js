'use strict';

const $ = (s, el = document) => el.querySelector(s);
const view = $('#view');
const titleEl = $('#title');
const backBtn = $('#back');
const langSel = $('#lang');

const store = {
  get(k, d) { try { return JSON.parse(localStorage.getItem(k)) ?? d; } catch { return d; } },
  set(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); } catch { /* private mode */ } },
};

let lang = store.get('lang', (navigator.language || 'es').slice(0, 2));
if (![...langSel.options].some(o => o.value === lang)) lang = 'es';
langSel.value = lang;
langSel.onchange = () => { lang = langSel.value; store.set('lang', lang); route(); };

const T = {
  es: {
    vin: 'Buscar por VIN', vinHint: '17 caracteres, en la ficha técnica o bajo el parabrisas', go: 'Buscar',
    pick: 'Elegir por modelo', recent: 'Vehículos recientes', noVin: 'Ningún manual reconoce ese VIN.',
    make: 'Marca', model: 'Modelo', year: 'Año', engine: 'Motor', transmission: 'Cambio',
    skip: 'No lo sé / todos', filter: 'Filtrar…', index: 'Índice', search: 'Buscar',
    searchHint: 'Buscar en los manuales de este vehículo', nothing: 'Nada por aquí.', change: 'Cambiar',
    sources: 'Manuales disponibles', results: 'resultados', related: 'El mismo vehículo en otros manuales',
  },
  en: {
    vin: 'Search by VIN', vinHint: '17 characters, on the registration papers or under the windscreen', go: 'Search',
    pick: 'Choose by model', recent: 'Recent vehicles', noVin: 'No manual recognises that VIN.',
    make: 'Make', model: 'Model', year: 'Year', engine: 'Engine', transmission: 'Transmission',
    skip: "Don't know / all", filter: 'Filter…', index: 'Contents', search: 'Search',
    searchHint: "Search this vehicle's manuals", nothing: 'Nothing here.', change: 'Change',
    sources: 'Available manuals', results: 'results', related: 'The same vehicle in other manuals',
  },
};
const t = k => (T[lang] || T.en)[k] ?? T.en[k];
const DOMAIN_NAME = { 2: 'make', 6: 'model', 3: 'year', 7: 'engine', 8: 'transmission' };

const api = (path, params = {}) => {
  const u = new URL(path, location.origin);
  u.searchParams.set('lang', lang);
  for (const [k, v] of Object.entries(params)) if (v != null && v !== '') u.searchParams.set(k, v);
  return fetch(u).then(r => { if (!r.ok) throw new Error(r.status); return r.json(); });
};

const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
const icon = {
  folder: '<svg class="ic" viewBox="0 0 24 24"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>',
  doc: '<svg class="ic" viewBox="0 0 24 24"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5M9 13h6M9 17h6"/></svg>',
  car: '<svg class="ic" viewBox="0 0 24 24"><path d="M5 17h14M3 13l2-5a2 2 0 0 1 2-1h10a2 2 0 0 1 2 1l2 5v4H3z"/><circle cx="7.5" cy="17" r="1.5"/><circle cx="16.5" cy="17" r="1.5"/></svg>',
  chev: '<svg class="chev" viewBox="0 0 24 24"><path d="M9 6l6 6-6 6"/></svg>',
};
const spinner = () => '<div class="spin"></div>';

// node labels seen, for breadcrumbs
const labels = store.get('labels', {});
const remember = (k, l) => { labels[k] = l; };
addEventListener('pagehide', () => {
  const keys = Object.keys(labels);
  if (keys.length > 3000) for (const k of keys.slice(0, keys.length - 3000)) delete labels[k];
  store.set('labels', labels);
});

function setTitle(s) { titleEl.textContent = s || ''; document.title = s ? `${s} · openmanuals` : 'openmanuals'; }

function recent() { return store.get('recent', []); }
function addRecent(v) {
  const r = recent().filter(x => !(x.src === v.src && x.ctx === v.ctx));
  r.unshift(v);
  store.set('recent', r.slice(0, 12));
}

async function vehicleLabel(src, ctx) {
  const key = `veh:${src}:${ctx}:${lang}`;
  if (labels[key]) return labels[key];
  const d = await api(`/api/s/${src}/describe`, { ctx });
  const l = d.map(x => x[1]).join(' · ');
  remember(key, l);
  return l;
}

// --- views -----------------------------------------------------------------

async function home() {
  setTitle('');
  const rec = recent();
  view.className = '';
  view.innerHTML = `
    <h2>${t('vin')}</h2>
    <form class="card" id="vinf">
      <div class="field">
        <input id="vin" maxlength="17" autocomplete="off" autocapitalize="characters" spellcheck="false" placeholder="W0L…">
        <button class="btn">${t('go')}</button>
      </div>
      <div class="muted" style="font-size:13px;margin-top:8px">${t('vinHint')}</div>
      <div id="vinres"></div>
    </form>
    <h2>${t('pick')}</h2>
    <ul class="list" id="srcs">${spinner()}</ul>
    ${rec.length ? `<h2>${t('recent')}</h2><ul class="list">${rec.map(r => `
      <li><a href="#/v/${r.src}/${encodeURIComponent(r.ctx)}">${icon.car}<span class="lbl">${esc(r.label)}<span class="sub">${esc(r.title || r.src)}</span></span>${icon.chev}</a></li>`).join('')}</ul>` : ''}`;
  $('#vinf').onsubmit = async e => {
    e.preventDefault();
    const vin = $('#vin').value.trim().toUpperCase();
    if (vin.length < 10) return;
    const out = $('#vinres');
    out.innerHTML = spinner();
    const res = await api(`/api/vin/${encodeURIComponent(vin)}`);
    if (!res.length) { out.innerHTML = `<p class="muted">${t('noVin')}</p>`; return; }
    if (res.length === 1) { location.hash = `#/v/${res[0].source}/${encodeURIComponent(res[0].ctx)}`; return; }
    out.innerHTML = `<ul class="list" style="margin-top:12px">${res.map(r => `
      <li><a href="#/v/${r.source}/${encodeURIComponent(r.ctx)}">${icon.car}<span class="lbl">${esc(r.label)}</span>${icon.chev}</a></li>`).join('')}</ul>`;
  };
  const srcs = await api('/api/sources');
  store.set('sources', srcs);
  $('#srcs').innerHTML = srcs.length ? srcs.map(s => `
    <li><a href="#/pick/${s.id}/">${icon.folder}<span class="lbl">${esc(s.title)}<span class="sub">${esc(s.release)}</span></span>${icon.chev}</a></li>`).join('')
    : `<li class="empty">${t('nothing')}</li>`;
}

async function pick(src, ctx) {
  view.className = '';
  view.innerHTML = spinner();
  const [opts, desc] = await Promise.all([api(`/api/s/${src}/options`, { ctx }), ctx ? api(`/api/s/${src}/describe`, { ctx }) : []]);
  const chosen = desc.map(x => x[1]);
  setTitle(chosen.join(' · '));
  const enough = ctx.split(',').filter(Boolean).length >= 3;
  if (!opts.length) { location.replace(`#/v/${src}/${encodeURIComponent(ctx)}`); return; }
  const dom = DOMAIN_NAME[opts[0].domain] || '';
  const next = v => (ctx ? ctx + ',' : '') + `${opts[0].domain}:${v}`;
  view.innerHTML = `
    ${chosen.length ? `<div class="chips">${chosen.map(c => `<span class="chip">${esc(c)}</span>`).join('')}</div>` : ''}
    <h1>${t(dom)}</h1>
    ${opts.length > 12 ? `<input class="filter card plain" style="width:100%;padding:10px 12px" placeholder="${t('filter')}" id="flt">` : ''}
    <ul class="list" id="opts">${opts.map(o => `
      <li data-l="${esc(o.label.toLowerCase())}"><a href="#/pick/${src}/${encodeURIComponent(next(o.value))}"><span class="lbl">${esc(o.label)}</span>${icon.chev}</a></li>`).join('')}</ul>
    ${enough ? `<p><a class="btn ghost" href="#/v/${src}/${encodeURIComponent(ctx)}">${t('skip')}</a></p>` : ''}`;
  const f = $('#flt');
  if (f) {
    f.focus();
    f.oninput = () => {
      const q = f.value.toLowerCase();
      for (const li of $('#opts').children) li.hidden = !li.dataset.l.includes(q);
    };
  }
}

function vehicleHeader(src, ctx, label, tab) {
  const base = `#/v/${src}/${encodeURIComponent(ctx)}`;
  return `
    <div class="chips"><span class="chip">${esc(label)}</span><a class="chip" href="#/pick/${src}/">${t('change')}</a></div>
    <nav class="tabs"><a href="${base}" class="${tab === 'index' ? 'on' : ''}">${t('index')}</a><a href="${base}/s/" class="${tab === 'search' ? 'on' : ''}">${t('search')}</a></nav>`;
}

function nodeList(src, ctx, path, nodes) {
  const enc = encodeURIComponent(ctx);
  if (!nodes.length) return `<div class="empty">${t('nothing')}</div>`;
  return `<ul class="list">${nodes.map(n => {
    remember(`n:${src}:${n.id}:${lang}`, n.label);
    if (n.doc) return `<li><a href="#/d/${src}/${enc}/${n.id}">${icon.doc}<span class="lbl">${esc(n.label)}</span></a></li>`;
    return `<li class="folder"><a href="#/v/${src}/${enc}/n/${[...path, n.id].join('.')}">${icon.folder}<span class="lbl">${esc(n.label)}</span>${icon.chev}</a></li>`;
  }).join('')}</ul>`;
}

async function vehicle(src, ctx, pathStr) {
  view.className = '';
  view.innerHTML = spinner();
  const label = await vehicleLabel(src, ctx);
  addRecent({ src, ctx, label, title: (store.get('sources', []).find(s => s.id === src) || {}).title });
  setTitle(label);
  const path = pathStr ? pathStr.split('.').map(Number) : [];
  const nodes = path.length
    ? await api(`/api/s/${src}/node/${path[path.length - 1]}`, { ctx })
    : await api(`/api/s/${src}/roots`, { ctx });
  const enc = encodeURIComponent(ctx);
  const crumbs = path.length ? `<nav class="crumbs"><a href="#/v/${src}/${enc}">${t('index')}</a>${path.map((id, i) =>
    `<span><a href="#/v/${src}/${enc}/n/${path.slice(0, i + 1).join('.')}">${esc(labels[`n:${src}:${id}:${lang}`] || '…')}</a></span>`).join('')}</nav>` : '';
  if (!path.length && nodes.length === 0) {
    view.innerHTML = vehicleHeader(src, ctx, label, 'index') + `<div class="empty">${t('nothing')}</div>`;
    return;
  }
  view.innerHTML = vehicleHeader(src, ctx, label, 'index') + crumbs + nodeList(src, ctx, path, nodes);
  if (!path.length) {
    const rel = await api(`/api/s/${src}/related`, { ctx }).catch(() => []);
    if (rel.length) {
      const titles = Object.fromEntries(store.get('sources', []).map(s => [s.id, s.title]));
      view.insertAdjacentHTML('beforeend', `<h2>${t('related')}</h2><ul class="list">${rel.map(r => `
        <li><a href="#/v/${r.source}/${encodeURIComponent(r.ctx)}">${icon.car}<span class="lbl">${esc(r.label)}<span class="sub">${esc(titles[r.source] || r.source)}</span></span>${icon.chev}</a></li>`).join('')}</ul>`);
    }
  }
}

async function search(src, ctx, q) {
  view.className = '';
  const label = await vehicleLabel(src, ctx);
  setTitle(label);
  view.innerHTML = vehicleHeader(src, ctx, label, 'search') + `
    <form id="sf" class="field" style="margin-bottom:12px"><input id="q" class="plain" placeholder="${t('searchHint')}" value="${esc(q)}"><button class="btn">${t('go')}</button></form>
    <div id="hits"></div>`;
  $('#sf').onsubmit = e => { e.preventDefault(); location.hash = `#/v/${src}/${encodeURIComponent(ctx)}/s/${encodeURIComponent($('#q').value)}`; };
  if (!q) { $('#q').focus(); return; }
  $('#hits').innerHTML = spinner();
  const hits = await api(`/api/s/${src}/search`, { ctx, q });
  const enc = encodeURIComponent(ctx);
  $('#hits').innerHTML = hits.length ? `<p class="muted">${hits.length} ${t('results')}</p><ul class="list hits">${hits.map(h => `
    <li><a href="#/d/${src}/${enc}/${h.id}">${icon.doc}<span class="lbl">${esc(h.title)}<span class="sub snip">${h.snippet.replace(/</g, '&lt;').replace(/&lt;(\/?)b>/g, '<$1b>')}</span></span></a></li>`).join('')}</ul>`
    : `<div class="empty">${t('nothing')}</div>`;
}

function docView(src, ctx, id, anchor) {
  view.className = 'full';
  setTitle(labels[`n:${src}:${id}:${lang}`] || '');
  const u = `/api/s/${src}/doc/${id}?lang=${lang}&ctx=${encodeURIComponent(ctx)}${anchor ? '#' + encodeURIComponent(anchor) : ''}`;
  view.innerHTML = `<iframe class="doc" src="${u}" title="document"></iframe>`;
  const fr = $('iframe', view);
  fr.onload = () => {
    try {
      const d = fr.contentDocument;
      const tt = d && d.title;
      if (tt && !titleEl.textContent) setTitle(tt.replace(/^\d+/, ''));
    } catch { /* cross-origin never happens here */ }
  };
}

// --- image viewer ------------------------------------------------------------

const viewer = $('#viewer');
const vimg = $('img', viewer);
let zoom = 1;
function applyZoom() {
  const s = viewer.querySelector('.viewer-stage');
  const w = s.clientWidth - 16, h = s.clientHeight - 16;
  const nw = vimg.naturalWidth || 1000, nh = vimg.naturalHeight || 700;
  const fit = Math.min(w / nw, h / nh);
  vimg.style.width = `${nw * fit * zoom}px`;
  vimg.style.height = `${nh * fit * zoom}px`;
}
function openImage(url) {
  zoom = 1;
  vimg.onload = applyZoom;
  vimg.src = url;
  viewer.querySelector('[data-act=open]').href = url;
  viewer.hidden = false;
}
viewer.onclick = e => {
  const a = e.target.closest('[data-act]');
  if (!a) return;
  const act = a.dataset.act;
  if (act === 'close') viewer.hidden = true;
  if (act === 'in') { zoom *= 1.5; applyZoom(); }
  if (act === 'out') { zoom = Math.max(0.25, zoom / 1.5); applyZoom(); }
  if (act === 'fit') { zoom = 1; applyZoom(); }
};
addEventListener('keydown', e => { if (e.key === 'Escape') viewer.hidden = true; });

// messages from the document frame (reader.js)
addEventListener('message', e => {
  if (e.origin !== location.origin || !e.data) return;
  const m = e.data;
  const r = parse();
  if (m.type === 'doc' && r.src) location.hash = `#/d/${r.src}/${encodeURIComponent(r.ctx || '')}/${m.id}${m.anchor ? '/' + encodeURIComponent(m.anchor) : ''}`;
  if (m.type === 'image') openImage(m.url);
});

// --- routing -----------------------------------------------------------------

function parse() {
  const h = location.hash.replace(/^#\/?/, '');
  const p = h.split('/');
  const dec = s => decodeURIComponent(s || '');
  if (p[0] === 'pick') return { v: 'pick', src: p[1], ctx: dec(p[2]) };
  if (p[0] === 'v') {
    if (p[3] === 'n') return { v: 'veh', src: p[1], ctx: dec(p[2]), path: p[4] };
    if (p[3] === 's') return { v: 'search', src: p[1], ctx: dec(p[2]), q: dec(p.slice(4).join('/')) };
    return { v: 'veh', src: p[1], ctx: dec(p[2]) };
  }
  if (p[0] === 'd') return { v: 'doc', src: p[1], ctx: dec(p[2]), id: p[3], anchor: dec(p[4]) };
  return { v: 'home' };
}

async function route() {
  viewer.hidden = true;
  const r = parse();
  backBtn.hidden = r.v === 'home';
  try {
    if (r.v === 'pick') await pick(r.src, r.ctx);
    else if (r.v === 'veh') await vehicle(r.src, r.ctx, r.path);
    else if (r.v === 'search') await search(r.src, r.ctx, r.q);
    else if (r.v === 'doc') docView(r.src, r.ctx, r.id, r.anchor);
    else await home();
  } catch (e) {
    view.innerHTML = `<div class="empty">Error: ${esc(e.message)}</div>`;
  }
  if (r.v !== 'doc') scrollTo(0, 0);
}
backBtn.onclick = () => history.back();
addEventListener('hashchange', route);
route();

if ('serviceWorker' in navigator) navigator.serviceWorker.register('/sw.js').catch(() => {});

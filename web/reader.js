// Runs inside every document page: shows only the variants that fit the chosen
// vehicle and hands links and pictures to the app around it.
'use strict';
(() => {
  const qs = new URLSearchParams(location.search);
  const lang = qs.get('lang') || 'es';
  const ctx = {};
  for (const p of (qs.get('ctx') || '').split(',')) {
    const [d, v] = p.split(':').map(Number);
    if (!Number.isNaN(d) && !Number.isNaN(v)) (ctx[d] ||= new Set()).add(v);
  }

  // `domain:value` expressions: and / or / ( ) / [ ]; an unknown domain does not exclude
  function allows(text) {
    const toks = text.match(/\(|\)|\[|\]|and|or|-?\d+:-?\d+/gi) || [];
    let i = 0;
    const or = () => { let v = and(); while (/^or$/i.test(toks[i] || '')) { i++; v = and() || v; } return v; };
    const and = () => { let v = atom(); while (/^and$/i.test(toks[i] || '')) { i++; v = atom() && v; } return v; };
    const atom = () => {
      const t = toks[i++];
      if (t === '(' || t === '[') { const v = or(); if (toks[i] === ')' || toks[i] === ']') i++; return v; }
      if (!t) return true;
      const [d, v] = t.split(':').map(Number);
      return !ctx[d] || ctx[d].has(v);
    };
    try { return or(); } catch { return true; }
  }

  function filterVariants() {
    // paragraphs that differ per vehicle: keep the matching ones when any match
    const groups = new Map();
    for (const el of document.querySelectorAll('[data-vcr]')) {
      const g = el.parentElement;
      if (!groups.has(g)) groups.set(g, []);
      groups.get(g).push(el);
    }
    for (const els of groups.values()) {
      const ok = els.map(el => allows(el.dataset.vcr));
      if (ok.some(Boolean)) els.forEach((el, k) => { if (!ok[k]) el.classList.add('om-hidden'); });
    }
  }

  const parent = window.parent !== window ? window.parent : null;
  function route(url) {
    const m = url.pathname.match(/^\/api\/s\/([^/]+)\/(doc|img|cell|lt)\/?(\w*)/);
    if (!m) return false;
    if (m[2] === 'doc') {
      if (Number(m[3]) === Number(location.pathname.split('/').pop()) && url.hash) return false;
      if (parent) { parent.postMessage({ type: 'doc', id: Number(m[3]), anchor: url.hash.slice(1) }, location.origin); return true; }
    }
    if (m[2] === 'img' && parent) { parent.postMessage({ type: 'image', url: url.pathname }, location.origin); return true; }
    if (m[2] === 'cell' || m[2] === 'lt') {
      url.searchParams.set('lang', lang);
      url.searchParams.set('ctx', qs.get('ctx') || '');
      location.href = url.href;
      return true;
    }
    return false;
  }

  // GlobalTIS link forms the pack builder may have left as they were
  function rawTarget(href) {
    let m = href.match(/^\{?LINK_SIO:ID:(\d+)(?::BM:([^}]*))?\}?$/);
    if (m) return new URL(`doc/${m[1]}${m[2] ? '#' + m[2] : ''}`, document.baseURI);
    m = href.match(/^\{LINK_CELL_TARGET:(.*)\}$/s);
    if (m) {
      const body = m[1].startsWith('LIST:') ? m[1].slice(5) : m[1];
      const targets = body.split(/\\?\//).filter(p => p.startsWith('ID:')).map(p => {
        const t = p.replace(/\\$/, '').match(/^ID:(\d+)(?::VCR:(.*))?$/s);
        return t ? `${t[1]}~${(t[2] || '').trim()}` : null;
      }).filter(Boolean);
      return new URL(`cell?c=${encodeURIComponent(targets.join(';'))}`, document.baseURI);
    }
    return null;
  }

  document.addEventListener('click', e => {
    const a = e.target.closest('a[href]');
    if (a) {
      const href = a.getAttribute('href');
      if (/^javascript:history\.(back|forward)/.test(href)) { e.preventDefault(); history[href.includes('back') ? 'back' : 'forward'](); return; }
      const url = rawTarget(href) || new URL(a.href, location.href);
      if (route(url)) e.preventDefault();
      else if (rawTarget(href)) { e.preventDefault(); location.href = url.href; }
      return;
    }
    const img = e.target.closest('img');
    if (img && /\/img\/\d+/.test(img.src) && parent) {
      parent.postMessage({ type: 'image', url: new URL(img.src).pathname }, location.origin);
    }
  });

  document.addEventListener('DOMContentLoaded', () => {
    filterVariants();
    for (const img of document.querySelectorAll('img[src*="/img/"], img.drawing')) img.classList.add('om-zoomable');
  });
})();

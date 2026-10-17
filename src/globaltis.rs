//! GlobalTIS packs (Opel, Vauxhall, Chevrolet Europe, Holden, Cadillac Europe).
//!
//! The vehicle is a set of `domain:value` pairs (make 2, model year 3, model 6,
//! engine 7, transmission 8); every tree node, document and paragraph carries a
//! restriction over the same domains (see `vcr`).

use crate::pack::{Doc, Hit, Lang, Node, Option_, Pack, VinMatch};
use crate::vcr::{Context, Expr};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

pub const MAKE: i64 = 2;
pub const YEAR: i64 = 3;
pub const VIN_YEAR: i64 = 4;
pub const VIN_WMI: i64 = 5;
pub const MODEL: i64 = 6;
pub const ENGINE: i64 = 7;
pub const TRANSMISSION: i64 = 8;
pub const LOCALE: i64 = 1;

/// Roots of the trees GlobalTIS offers: per model and year by assembly group,
/// and the whole catalogue by kind of information.
const ROOT_BY_MODEL: i64 = -1;
const ROOT_BY_TYPE: i64 = -7;

pub struct GlobalTis {
    pool: Vec<Mutex<Connection>>,
    next: std::sync::atomic::AtomicUsize,
    vcr: OnceLock<HashMap<i64, Arc<Expr>>>,
    locales: HashMap<String, i64>,
}

impl GlobalTis {
    pub fn open(path: &std::path::Path) -> rusqlite::Result<Self> {
        let n = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
        let mut pool = Vec::new();
        for _ in 0..n {
            let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            c.pragma_update(None, "mmap_size", 1i64 << 30)?;
            pool.push(Mutex::new(c));
        }
        let locales = {
            let c = pool[0].lock().unwrap();
            let mut s = c.prepare("SELECT code, id FROM locale")?;
            s.query_map([], |r| Ok((r.get::<_, String>(0)?[..2].to_string(), r.get(1)?)))?
                .filter_map(Result::ok)
                .collect()
        };
        Ok(Self { pool, next: Default::default(), vcr: OnceLock::new(), locales })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        let i = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % self.pool.len();
        self.pool[i].lock().unwrap()
    }

    fn vcr(&self, id: i64) -> Option<Arc<Expr>> {
        if id < 0 {
            return None;
        }
        let map = self.vcr.get_or_init(|| {
            let c = self.conn();
            let mut s = c.prepare("SELECT id, text FROM vcr").unwrap();
            s.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
                .unwrap()
                .filter_map(Result::ok)
                .filter_map(|(i, t)| Some((i, Arc::new(Expr::parse(&t)?))))
                .collect()
        });
        map.get(&id).cloned()
    }

    fn allows(&self, vcr: i64, ctx: &Context) -> bool {
        self.vcr(vcr).is_none_or(|e| e.allows(ctx))
    }

    /// Locales to try for a language, then English, then German.
    fn chain(&self, lang: &Lang) -> Vec<i64> {
        let mut v = Vec::new();
        for l in [lang.0.as_str(), "en", "de"] {
            if let Some(id) = self.locales.get(l)
                && !v.contains(id)
            {
                v.push(*id);
            }
        }
        v
    }

    fn label(&self, c: &Connection, domain: i64, value: i64, lang: &Lang) -> Option<String> {
        let mut ls = vec![0i64];
        ls.extend(self.chain(lang));
        for l in ls {
            let r: Option<String> = c
                .query_row(
                    "SELECT l.label FROM vc_value v JOIN vc_label l ON l.id = v.label AND l.lang = ?3 \
                     WHERE v.domain = ?1 AND v.id = ?2",
                    params![domain, value, l],
                    |r| r.get(0),
                )
                .optional()
                .ok()
                .flatten();
            if r.is_some() {
                return r;
            }
        }
        None
    }

    fn with_locale(&self, ctx: &Context, lang: &Lang) -> Context {
        match self.chain(lang).first() {
            Some(l) => ctx.with(LOCALE, *l),
            None => ctx.clone(),
        }
    }

    fn toc_label(&self, c: &Connection, label: i64, lang: &Lang) -> Option<String> {
        for l in self.chain(lang) {
            let r: Option<String> = c
                .query_row("SELECT label FROM toc_label WHERE locale = ?1 AND id = ?2", params![l, label], |r| r.get(0))
                .optional()
                .ok()
                .flatten();
            if r.is_some() {
                return r;
            }
        }
        None
    }

    fn subject(&self, c: &Connection, doc: i64, lang: &Lang) -> Option<String> {
        for l in self.chain(lang) {
            let r: Option<String> = c
                .query_row("SELECT subject FROM doc_subject WHERE id = ?1 AND locale = ?2", params![doc, l], |r| r.get(0))
                .optional()
                .ok()
                .flatten();
            if r.is_some() {
                return r;
            }
        }
        None
    }

    /// The per-model tree for this vehicle (make, model and year must be known).
    fn model_root(&self, c: &Connection, ctx: &Context) -> Option<i64> {
        let mut s = c.prepare_cached("SELECT id, vcr FROM toc WHERE parent = ?1").ok()?;
        let rows: Vec<(i64, i64)> =
            s.query_map([ROOT_BY_MODEL], |r| Ok((r.get(0)?, r.get(1)?))).ok()?.filter_map(Result::ok).collect();
        let strict = |e: &Expr| matches_all(e, ctx);
        rows.iter().find(|(_, v)| self.vcr(*v).is_some_and(|e| strict(&e))).map(|r| r.0)
    }
}

/// Every atom of the expression's first conjunction is satisfied by a value the
/// vehicle actually has (used to pick a model root, where "unknown" must not match).
fn matches_all(e: &Expr, ctx: &Context) -> bool {
    match e {
        Expr::Atom(d, v) => ctx.get(*d).is_some_and(|s| s.contains(v)),
        Expr::And(xs) => xs.iter().all(|x| matches_all(x, ctx)),
        Expr::Or(xs) => xs.iter().any(|x| matches_all(x, ctx)),
    }
}

fn glob(pattern: &str, s: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = s.chars().collect();
    fn go(p: &[char], t: &[char]) -> bool {
        match p.first() {
            None => true, // patterns are prefixes
            Some('*') => (0..=t.len()).any(|i| go(&p[1..], &t[i..])),
            Some('?') => !t.is_empty() && go(&p[1..], &t[1..]),
            Some(c) => t.first() == Some(c) && go(&p[1..], &t[1..]),
        }
    }
    go(&p, &t)
}

impl Pack for GlobalTis {
    fn options(&self, ctx: &Context, lang: &Lang) -> Vec<Option_> {
        // make -> model -> year -> engine -> transmission, from the vehicle configurations
        let c = self.conn();
        let order = [MAKE, MODEL, YEAR, ENGINE, TRANSMISSION];
        let Some(&next) = order.iter().find(|d| ctx.get(**d).is_none()) else { return vec![] };
        let mut vals: BTreeMap<i64, ()> = BTreeMap::new();
        let mut s = c.prepare_cached("SELECT config, domain, value FROM vc_config ORDER BY config").unwrap();
        let mut configs: BTreeMap<i64, Vec<(i64, i64)>> = BTreeMap::new();
        for r in s.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))).unwrap().flatten() {
            configs.entry(r.0).or_default().push((r.1, r.2));
        }
        let mut a = c.prepare_cached("SELECT domain, value FROM vc_assoc WHERE config = ?1").unwrap();
        for (id, kv) in &configs {
            let fits = kv.iter().all(|(d, v)| ctx.get(*d).is_none_or(|s| s.contains(v)));
            if !fits {
                continue;
            }
            if let Some((_, v)) = kv.iter().find(|(d, _)| *d == next) {
                vals.insert(*v, ());
            } else if next == ENGINE || next == TRANSMISSION {
                for (d, v) in a.query_map([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).unwrap().flatten() {
                    if d == next {
                        vals.insert(v, ());
                    }
                }
            }
        }
        let mut out: Vec<Option_> = vals
            .keys()
            .map(|v| Option_ {
                domain: next,
                value: *v,
                label: self.label(&c, next, *v, lang).unwrap_or_else(|| v.to_string()),
            })
            .collect();
        if next == YEAR {
            out.reverse();
        } else {
            out.sort_by(|x, y| x.label.cmp(&y.label));
        }
        out
    }

    fn describe(&self, ctx: &Context, lang: &Lang) -> Vec<(i64, String)> {
        let c = self.conn();
        let mut out = Vec::new();
        for d in [MAKE, MODEL, YEAR, ENGINE, TRANSMISSION] {
            if let Some(set) = ctx.get(d) {
                for v in set {
                    out.push((d, self.label(&c, d, *v, lang).unwrap_or_else(|| v.to_string())));
                }
            }
        }
        out
    }

    fn vin(&self, vin: &str, lang: &Lang) -> Vec<VinMatch> {
        let vin = vin.trim().to_ascii_uppercase();
        if vin.len() < 10 {
            return vec![];
        }
        let c = self.conn();
        let value_of = |domain: i64, label: &str| -> Option<i64> {
            c.query_row(
                "SELECT v.id FROM vc_value v JOIN vc_label l ON l.id = v.label AND l.lang = 0 \
                 WHERE v.domain = ?1 AND l.label = ?2",
                params![domain, label],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
        };
        let Some(wmi) = value_of(VIN_WMI, &vin[..3]) else { return vec![] };
        let no_year: Vec<String> = c
            .prepare("SELECT pattern FROM vin_no_model_year")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .flatten()
            .collect();
        let years: Vec<i64> = if no_year.iter().any(|p| glob(p, &vin)) {
            vec![]
        } else {
            value_of(VIN_YEAR, &vin[9..10]).into_iter().collect()
        };
        let mut s = c
            .prepare(
                "SELECT structure, make, model_year, domain, pos_from, pos_to, pattern FROM vin_structure \
                 WHERE wmi = ?1 ORDER BY structure",
            )
            .unwrap();
        let mut by_structure: BTreeMap<i64, Vec<(i64, i64, i64, usize, usize, Option<String>)>> = BTreeMap::new();
        for r in s
            .query_map([wmi], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?,
                    r.get::<_, usize>(4)?, r.get::<_, usize>(5)?, r.get::<_, Option<String>>(6)?))
            })
            .unwrap()
            .flatten()
        {
            if years.is_empty() || years.contains(&r.2) {
                by_structure.entry(r.0).or_default().push((r.1, r.2, r.3, r.4, r.5, r.6));
            }
        }
        let mut el = c.prepare("SELECT value FROM vin_element WHERE structure = ?1 AND element = ?2 AND domain = ?3").unwrap();
        // one candidate per make and year
        let mut found: BTreeMap<(i64, i64), Context> = BTreeMap::new();
        for (st, rows) in &by_structure {
            let window = |f: usize, t: usize| vin.get(f - 1..t).unwrap_or("");
            // a pattern constrains the vehicle descriptor, from position 4 on (`P?2*`),
            // whichever row carries it
            if rows.iter().any(|r| r.5.as_deref().is_some_and(|p| !glob(p, &vin[3..]))) {
                continue;
            }
            for (make, year, domain, f, t, _) in rows {
                let w = window(*f, *t);
                let v: Option<i64> = el.query_row(params![st, w, domain], |r| r.get(0)).optional().ok().flatten();
                if let Some(v) = v {
                    let e = found.entry((*make, *year)).or_insert_with(|| {
                        let mut c = Context::default();
                        c.add(MAKE, *make);
                        c.add(YEAR, *year);
                        c
                    });
                    if e.get(*domain).is_none() {
                        e.add(*domain, v);
                    }
                }
            }
        }
        drop(el);
        drop(s);
        drop(c);
        found
            .into_values()
            .filter(|ctx| ctx.get(MODEL).is_some())
            .map(|ctx| VinMatch { label: self.describe(&ctx, lang).into_iter().map(|x| x.1).collect::<Vec<_>>().join(" · "), ctx })
            .collect()
    }

    fn labour(&self, op: &str, ctx: &Context, lang: &Lang) -> Option<String> {
        let c = self.conn();
        let model = ctx.get(MODEL).and_then(|s| s.iter().next().copied()).and_then(|v| self.label(&c, MODEL, v, lang))?;
        let mcs: Vec<i64> = c
            .prepare_cached("SELECT DISTINCT mc FROM lt_catalog WHERE model = ?1")
            .ok()?
            .query_map([&model], |r| r.get(0))
            .ok()?
            .flatten()
            .collect();
        let lc_of = |code: &str| -> Option<i64> {
            c.query_row("SELECT lc FROM lt_lang WHERE code = ?1", [code], |r| r.get(0)).optional().ok().flatten()
        };
        let lcs: Vec<i64> = [lang.0.as_str(), "en", "de"].iter().filter_map(|l| lc_of(l)).collect();
        let engine = ctx.get(ENGINE).and_then(|s| s.iter().next().copied()).and_then(|v| self.label(&c, ENGINE, v, lang));
        let base: String = op.chars().take(7).collect();
        let text = |mc: i64, nr: &str, table: &str| -> Option<String> {
            lcs.iter().find_map(|lc| {
                c.query_row(
                    &format!("SELECT text FROM {table} WHERE mc = ?1 AND nr = ?2 AND lc = ?3"),
                    params![mc, nr, lc],
                    |r| r.get::<_, String>(0),
                )
                .optional()
                .ok()
                .flatten()
                .filter(|t| !t.trim().is_empty())
            })
        };
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;");
        let mut rows = String::new();
        let mut title = None;
        for mc in &mcs {
            title = title.or_else(|| text(*mc, &base, "lt_text"));
            let mut s = c
                .prepare_cached("SELECT nr, minutes FROM lt_time WHERE mc = ?1 AND nr LIKE ?2 ORDER BY nr")
                .ok()?;
            let ops: Vec<(String, i64)> =
                s.query_map(params![mc, format!("{base}%")], |r| Ok((r.get(0)?, r.get(1)?))).ok()?.flatten().collect();
            for (nr, min) in ops {
                let name = text(*mc, &nr, "lt_text").unwrap_or_default();
                let note = text(*mc, &nr, "lt_desc").map(|d| format!("<div class=\"note\">{}</div>", esc(&d))).unwrap_or_default();
                let time = if min >= 0 {
                    format!("{:.1} h<div class=\"note\">{} min · {} AW</div>", min as f64 / 60.0, min, (min as f64 / 5.0).round())
                } else {
                    "—".to_string()
                };
                let mark = if nr.starts_with(op) && nr.len() == op.len() { " class=\"this\"" } else { "" };
                // times by special key (engine, gearbox, equipment)
                let mut sx = c.prepare_cached("SELECT sx, minutes FROM lt_sx WHERE mc = ?1 AND nr = ?2").ok()?;
                let variants: Vec<(String, i64)> =
                    sx.query_map(params![mc, nr], |r| Ok((r.get(0)?, r.get(1)?))).ok()?.flatten().collect();
                if variants.is_empty() || min > 0 {
                    rows.push_str(&format!("<tr{mark}><td class=\"nr\">{}</td><td>{}{}</td><td class=\"t\">{}</td></tr>", esc(&nr), esc(&name), note, time));
                } else {
                    rows.push_str(&format!("<tr{mark}><td class=\"nr\">{}</td><td colspan=\"2\">{}{}</td></tr>", esc(&nr), esc(&name), note));
                }
                for (key, m) in variants {
                    let cond = lcs
                        .iter()
                        .find_map(|lc| {
                            c.query_row("SELECT text FROM lt_sx_text WHERE sx = ?1 AND lc = ?2", params![key, lc], |r| r.get::<_, String>(0))
                                .optional()
                                .ok()
                                .flatten()
                        })
                        .unwrap_or(key);
                    let mine = engine.as_deref().is_some_and(|e| cond.contains(e));
                    rows.push_str(&format!(
                        "<tr class=\"sx{}\"><td></td><td>{}</td><td class=\"t\">{:.1} h<div class=\"note\">{} min</div></td></tr>",
                        if mine { " this" } else { "" },
                        esc(&cond),
                        m as f64 / 60.0,
                        m
                    ));
                }
            }
        }
        if rows.is_empty() {
            return None;
        }
        let title = title.unwrap_or_else(|| op.to_string());
        Some(format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>{t}</title><style>             body{{font:16px/1.45 system-ui,sans-serif}}table{{border-collapse:collapse;width:100%}}             td{{padding:6px;border-bottom:1px solid #e3e6ea;vertical-align:top}}.nr{{font-family:monospace;white-space:nowrap}}             .t{{text-align:right;font-weight:600;white-space:nowrap}}.note{{color:#667085;font-size:13px;font-weight:400}}             tr.this td{{background:#e7f0ff;font-weight:600}}tr.sx td{{font-size:14px}}</style></head><body><h1>{t}</h1><p class=\"note\">{m} · {op}</p>             <table>{rows}</table></body></html>",
            t = esc(&title),
            m = esc(&model),
        ))
    }

    fn keys(&self, ctx: &Context, lang: &Lang) -> crate::pack::Keys {
        let c = self.conn();
        let one = |d: i64| ctx.get(d).and_then(|s| s.iter().next().copied()).and_then(|v| self.label(&c, d, v, lang));
        // "ASTRA-J" -> "ASTRA"; "1.7L (LPV)" -> "LPV"
        let model = one(MODEL).unwrap_or_default().split(['-', ' ', '/']).next().unwrap_or("").to_string();
        let engines = one(ENGINE)
            .and_then(|e| Some(e[e.find('(')? + 1..e.find(')')?].trim().to_string()))
            .into_iter()
            .collect();
        crate::pack::Keys { model, engines }
    }

    fn roots(&self, ctx: &Context, lang: &Lang) -> Vec<Node> {
        let c = self.conn();
        let mut out = Vec::new();
        if let Some(r) = self.model_root(&c, ctx) {
            out.push(Node { id: r, label: tr(lang, "By assembly group", "Por grupo"), folder: true, doc: false });
        }
        let by_type: Option<i64> =
            c.query_row("SELECT id FROM toc WHERE parent = ?1", [ROOT_BY_TYPE], |r| r.get(0)).optional().ok().flatten();
        if let Some(r) = by_type {
            out.push(Node { id: r, label: tr(lang, "By kind of information", "Por tipo de información"), folder: true, doc: false });
        }
        out
    }

    fn children(&self, node: i64, ctx: &Context, lang: &Lang) -> Vec<Node> {
        let ctx = self.with_locale(ctx, lang);
        let c = self.conn();
        let mut out = Vec::new();
        let mut s = c
            .prepare_cached("SELECT id, label, has_children, has_content, vcr FROM toc WHERE parent = ?1 ORDER BY ord")
            .unwrap();
        let rows: Vec<(i64, i64, bool, bool, i64)> = s
            .query_map([node], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .unwrap()
            .flatten()
            .collect();
        for (id, label, kids, content, vcr) in rows {
            if !self.allows(vcr, &ctx) || !(kids || content) {
                continue;
            }
            if let Some(l) = self.toc_label(&c, label, lang) {
                out.push(Node { id, label: l, folder: true, doc: false });
            }
        }
        let mut s = c.prepare_cached("SELECT doc, vcr FROM toc_content WHERE toc = ?1 ORDER BY ord").unwrap();
        let docs: Vec<(i64, i64)> =
            s.query_map([node], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().flatten().collect();
        let mut seen = std::collections::HashSet::new();
        for (doc, vcr) in docs {
            if !seen.insert(doc) || !self.allows(vcr, &ctx) {
                continue;
            }
            if let Some(l) = self.subject(&c, doc, lang) {
                out.push(Node { id: doc, label: l, folder: false, doc: true });
            }
        }
        out
    }

    fn doc(&self, id: i64, lang: &Lang) -> Option<Doc> {
        let c = self.conn();
        for l in self.chain(lang) {
            let html: Option<Vec<u8>> = c
                .query_row("SELECT html FROM doc WHERE id = ?1 AND locale = ?2", params![id, l], |r| r.get(0))
                .optional()
                .ok()
                .flatten();
            if let Some(html) = html {
                let title = self.subject(&c, id, lang).unwrap_or_default();
                return Some(Doc { title, gzip_html: html });
            }
        }
        None
    }

    fn image(&self, id: i64) -> Option<(String, bool, Vec<u8>)> {
        self.conn()
            .query_row("SELECT mime, gzip, data FROM img WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()
            .ok()
            .flatten()
    }

    fn static_file(&self, name: &str) -> Option<(String, Vec<u8>)> {
        self.conn()
            .query_row("SELECT mime, data FROM static WHERE name = ?1", [name.to_ascii_lowercase()], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
            .ok()
            .flatten()
    }

    fn cell(&self, targets: &str, ctx: &Context, lang: &Lang) -> Option<i64> {
        let ctx = self.with_locale(ctx, lang);
        let mut first = None;
        for t in targets.split(';') {
            let (id, expr) = t.split_once('~').unwrap_or((t, ""));
            let Ok(id) = id.parse::<i64>() else { continue };
            first.get_or_insert(id);
            if expr.is_empty() || Expr::parse(expr).is_none_or(|e| e.allows(&ctx)) {
                return Some(id);
            }
        }
        first
    }

    fn search(&self, q: &str, ctx: &Context, lang: &Lang, limit: usize) -> Vec<Hit> {
        let ctx = self.with_locale(ctx, lang);
        let c = self.conn();
        let Some(&loc) = self.chain(lang).first() else { return vec![] };
        let query = fts_query(q);
        if query.is_empty() {
            return vec![];
        }
        let mut s = c
            .prepare_cached(
                "SELECT rowid, snippet(fts, 1, '<b>', '</b>', '…', 16) FROM fts WHERE fts MATCH ?1 \
                 AND (rowid & 31) = ?2 ORDER BY rank LIMIT 2000",
            )
            .unwrap();
        let rows: Vec<i64> = s.query_map(params![query, loc], |r| r.get(0)).unwrap().flatten().collect();
        let mut vcrs = c.prepare_cached("SELECT vcr FROM toc_content WHERE doc = ?1").unwrap();
        let mut out = Vec::new();
        let words = search_words(q);
        for rowid in rows {
            let id = rowid >> 5;
            let v: Vec<i64> = vcrs.query_map([id], |r| r.get(0)).unwrap().flatten().collect();
            if !v.is_empty() && !v.iter().any(|x| self.allows(*x, &ctx)) {
                continue;
            }
            let title = self.subject(&c, id, lang).unwrap_or_default();
            let html: Option<Vec<u8>> = c
                .query_row("SELECT html FROM doc WHERE id = ?1 AND locale = ?2", params![id, loc], |r| r.get(0))
                .optional()
                .ok()
                .flatten();
            let snippet = html.map(|h| snippet(&h, &words)).unwrap_or_default();
            out.push(Hit { id, title, snippet });
            if out.len() >= limit {
                break;
            }
        }
        out
    }
}

fn tr(lang: &Lang, en: &str, es: &str) -> String {
    if lang.0 == "es" { es } else { en }.to_string()
}

/// The searched words, folded.
pub fn search_words(q: &str) -> Vec<String> {
    q.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(fold).collect()
}

/// Lower case without accents, as the index sees words.
pub fn fold(w: &str) -> String {
    w.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            'ß' => 's',
            c => c,
        })
        .collect()
}

/// A few words of the document around the first searched word, with the
/// searched words in `<b>`.
pub fn snippet(gz: &[u8], words: &[String]) -> String {
    use std::io::Read;
    let mut html = String::new();
    if flate2::read::GzDecoder::new(gz).read_to_string(&mut html).is_err() {
        return String::new();
    }
    let body = html.find("<body").or_else(|| html.find("<BODY")).map_or(&html[..], |i| &html[i..]);
    let mut text = String::new();
    let mut in_tag = false;
    for ch in body.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = entities(&text);
    let toks: Vec<&str> = text.split_whitespace().collect();
    let hit = |t: &str| {
        let f = fold(t.trim_matches(|c: char| !c.is_alphanumeric()));
        words.iter().any(|w| f.starts_with(w.as_str()))
    };
    let at = toks.iter().position(|t| hit(t)).unwrap_or(0);
    let from = at.saturating_sub(8);
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    for t in toks.iter().skip(from).take(28) {
        let t = t.replace('<', "&lt;");
        if hit(&t) {
            out.push_str(&format!("<b>{t}</b> "));
        } else {
            out.push_str(&t);
            out.push(' ');
        }
    }
    out.trim_end().to_string()
}

/// Decodes `&#NNN;`, `&#xHH;` and the Latin-1 named entities the manuals use.
fn entities(s: &str) -> String {
    const NAMED: &[(&str, char)] = &[
        ("nbsp", ' '), ("amp", '&'), ("lt", '<'), ("gt", '>'), ("quot", '"'), ("deg", '°'), ("ndash", '–'),
        ("mdash", '—'), ("szlig", 'ß'), ("iquest", '¿'), ("iexcl", '¡'), ("ordm", 'º'), ("ordf", 'ª'),
        ("aacute", 'á'), ("eacute", 'é'), ("iacute", 'í'), ("oacute", 'ó'), ("uacute", 'ú'), ("ntilde", 'ñ'),
        ("Aacute", 'Á'), ("Eacute", 'É'), ("Iacute", 'Í'), ("Oacute", 'Ó'), ("Uacute", 'Ú'), ("Ntilde", 'Ñ'),
        ("auml", 'ä'), ("ouml", 'ö'), ("uuml", 'ü'), ("Auml", 'Ä'), ("Ouml", 'Ö'), ("Uuml", 'Ü'),
        ("agrave", 'à'), ("egrave", 'è'), ("ccedil", 'ç'), ("atilde", 'ã'), ("otilde", 'õ'), ("acirc", 'â'),
        ("ecirc", 'ê'), ("ocirc", 'ô'),
    ];
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let end = after.find(';').filter(|e| *e <= 8);
        let decoded = end.and_then(|e| {
            let name = &after[..e];
            if let Some(n) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(n, 16).ok().and_then(char::from_u32)
            } else if let Some(n) = name.strip_prefix('#') {
                n.parse().ok().and_then(char::from_u32)
            } else {
                NAMED.iter().find(|(k, _)| *k == name).map(|x| x.1)
            }
        });
        match (decoded, end) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = &after[e + 1..];
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// User words to an FTS5 query: every word, prefix-matched.
pub fn fts_query(q: &str) -> String {
    q.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::glob;

    #[test]
    fn globs() {
        assert!(glob("P*", "P"));
        assert!(glob("MA6J?????9", "MA6JXXXXX9ABCDEF"));
        assert!(!glob("W0LVR", "W0LVS1"));
    }
}

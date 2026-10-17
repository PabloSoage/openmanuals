//! Packs that are already shaped the way the app reads: a vehicle hierarchy
//! (make → model → version), a contents tree per vehicle and ready HTML pages.
//! Builders for sources whose data is not organised as documents (HaynesPro
//! WorkshopData, Autodata) render their pages into this form.
//!
//! The vehicle context names one vehicle per level: domain 1 make, 2 model,
//! 3 version, each value a `vehicle.id`.

use crate::pack::{Doc, Hit, Lang, Node, Option_, Pack, VinMatch};
use crate::vcr::Context;
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::Mutex;

const LEVELS: [i64; 3] = [1, 2, 3];

pub struct Tree {
    pool: Vec<Mutex<Connection>>,
    next: std::sync::atomic::AtomicUsize,
}

impl Tree {
    pub fn open(path: &std::path::Path) -> rusqlite::Result<Self> {
        let n = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
        let mut pool = Vec::new();
        for _ in 0..n {
            let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            c.pragma_update(None, "mmap_size", 1i64 << 30)?;
            pool.push(Mutex::new(c));
        }
        Ok(Self { pool, next: Default::default() })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        let i = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % self.pool.len();
        self.pool[i].lock().unwrap()
    }

    /// The most specific vehicle the context names.
    fn vehicle(ctx: &Context) -> Option<i64> {
        LEVELS.iter().rev().find_map(|d| ctx.get(*d).and_then(|s| s.iter().next().copied()))
    }
}

impl Pack for Tree {
    fn options(&self, ctx: &Context, _lang: &Lang) -> Vec<Option_> {
        let Some(&level) = LEVELS.iter().find(|d| ctx.get(**d).is_none()) else { return vec![] };
        let parent = Self::vehicle(ctx).unwrap_or(0);
        let c = self.conn();
        let mut s = c
            .prepare_cached("SELECT id, label, sub FROM vehicle WHERE parent = ?1 ORDER BY ord, label")
            .unwrap();
        s.query_map([parent], |r| {
            let label: String = r.get(1)?;
            let sub: Option<String> = r.get(2)?;
            Ok(Option_ {
                domain: level,
                value: r.get(0)?,
                label: match sub.filter(|s| !s.is_empty()) {
                    Some(s) => format!("{label} · {s}"),
                    None => label,
                },
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    fn describe(&self, ctx: &Context, _lang: &Lang) -> Vec<(i64, String)> {
        let c = self.conn();
        LEVELS
            .iter()
            .filter_map(|d| {
                let v = *ctx.get(*d)?.iter().next()?;
                let l: String = c.query_row("SELECT label FROM vehicle WHERE id = ?1", [v], |r| r.get(0)).ok()?;
                Some((*d, l))
            })
            .collect()
    }

    fn vin(&self, _vin: &str, _lang: &Lang) -> Vec<VinMatch> {
        vec![]
    }

    fn roots(&self, ctx: &Context, lang: &Lang) -> Vec<Node> {
        match Self::vehicle(ctx) {
            Some(v) if ctx.get(3).is_some() => self.children_of(v, 0, lang),
            _ => vec![],
        }
    }

    fn children(&self, node: i64, ctx: &Context, lang: &Lang) -> Vec<Node> {
        match Self::vehicle(ctx) {
            Some(v) => self.children_of(v, node, lang),
            None => vec![],
        }
    }

    fn doc(&self, id: i64, _lang: &Lang) -> Option<Doc> {
        self.conn()
            .query_row("SELECT title, html FROM doc WHERE id = ?1", [id], |r| {
                Ok(Doc { title: r.get(0)?, gzip_html: r.get(1)? })
            })
            .optional()
            .ok()
            .flatten()
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
            .query_row("SELECT mime, data FROM static WHERE name = ?1", [name], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .ok()
            .flatten()
    }

    fn cell(&self, targets: &str, _ctx: &Context, _lang: &Lang) -> Option<i64> {
        targets.split(';').next()?.split('~').next()?.parse().ok()
    }

    fn find(&self, keys: &crate::pack::Keys, _lang: &Lang) -> Vec<VinMatch> {
        if keys.model.is_empty() || keys.engines.is_empty() {
            return vec![];
        }
        let c = self.conn();
        let mut s = c
            .prepare_cached(
                "SELECT t.id, t.label, t.sub, m.id, m.label, k.id, k.label FROM vehicle_code vc                  JOIN vehicle t ON t.id = vc.vehicle JOIN vehicle m ON m.id = t.parent JOIN vehicle k ON k.id = m.parent                  WHERE vc.code LIKE ?1 AND upper(m.label) LIKE ?2 LIMIT 20",
            )
            .unwrap();
        let mut out = Vec::new();
        for e in &keys.engines {
            let rows = s
                .query_map(params![format!("%({e})%"), format!("{}%", keys.model.to_uppercase())], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?,
                        r.get::<_, String>(4)?, r.get::<_, i64>(5)?, r.get::<_, String>(6)?))
                })
                .unwrap()
                .flatten();
            for (t, tl, ts, m, ml, k, kl) in rows {
                let mut ctx = Context::default();
                ctx.add(1, k);
                ctx.add(2, m);
                ctx.add(3, t);
                let label = format!("{kl} · {ml} · {tl}{}", ts.map(|s| format!(" · {s}")).unwrap_or_default());
                out.push(VinMatch { label, ctx });
            }
        }
        out
    }

    fn search(&self, q: &str, ctx: &Context, _lang: &Lang, limit: usize) -> Vec<Hit> {
        let Some(v) = Self::vehicle(ctx) else { return vec![] };
        let query = crate::globaltis::fts_query(q);
        if query.is_empty() {
            return vec![];
        }
        let c = self.conn();
        let mut s = c
            .prepare_cached(
                "SELECT d.id, d.title, d.html FROM fts JOIN doc_vehicle dv ON dv.doc = fts.rowid AND dv.vehicle = ?2 \
                 JOIN doc d ON d.id = fts.rowid WHERE fts MATCH ?1 ORDER BY rank LIMIT ?3",
            )
            .unwrap();
        let words: Vec<String> = crate::globaltis::search_words(q);
        s.query_map(params![query, v, limit as i64], |r| {
            let html: Vec<u8> = r.get(2)?;
            Ok(Hit { id: r.get(0)?, title: r.get(1)?, snippet: crate::globaltis::snippet(&html, &words) })
        })
        .unwrap()
        .flatten()
        .collect()
    }
}

impl Tree {
    fn children_of(&self, vehicle: i64, parent: i64, _lang: &Lang) -> Vec<Node> {
        let c = self.conn();
        let mut s = c
            .prepare_cached(
                "SELECT id, label, doc, EXISTS(SELECT 1 FROM node k WHERE k.parent = n.id) FROM node n \
                 WHERE (vehicle = ?1 OR vehicle IS NULL) AND parent = ?2 ORDER BY ord, label",
            )
            .unwrap();
        s.query_map(params![vehicle, parent], |r| {
            let doc: Option<i64> = r.get(2)?;
            let kids: bool = r.get(3)?;
            Ok(match doc {
                Some(d) if !kids => Node { id: d, label: r.get(1)?, folder: false, doc: true },
                _ => Node { id: r.get(0)?, label: r.get(1)?, folder: true, doc: false },
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }
}

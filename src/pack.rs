//! What every source of manuals offers the web app.

use crate::vcr::Context;
use serde::Serialize;

/// Two-letter language the reader asked for (`es`, `en`, `de`, `pt`…).
#[derive(Debug, Clone)]
pub struct Lang(pub String);

/// A next choice while narrowing the vehicle down.
#[derive(Debug, Serialize)]
pub struct Option_ {
    pub domain: i64,
    pub value: i64,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct VinMatch {
    pub label: String,
    #[serde(serialize_with = "ser_ctx")]
    pub ctx: Context,
}

#[derive(Debug, Serialize)]
pub struct Node {
    pub id: i64,
    pub label: String,
    pub folder: bool,
    pub doc: bool,
}

#[derive(Debug, Serialize)]
pub struct Hit {
    pub id: i64,
    pub title: String,
    pub snippet: String,
}

pub struct Doc {
    pub title: String,
    pub gzip_html: Vec<u8>,
}

fn ser_ctx<S: serde::Serializer>(c: &Context, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&c.to_string())
}

impl std::fmt::Display for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        for d in 0..64 {
            if let Some(set) = self.get(d) {
                let mut v: Vec<_> = set.iter().collect();
                v.sort();
                parts.extend(v.into_iter().map(|v| format!("{d}:{v}")));
            }
        }
        f.write_str(&parts.join(","))
    }
}

pub trait Pack: Send + Sync {
    /// Next choices for a vehicle known this far.
    fn options(&self, ctx: &Context, lang: &Lang) -> Vec<Option_>;
    /// Names of what the context pins down, in display order.
    fn describe(&self, ctx: &Context, lang: &Lang) -> Vec<(i64, String)>;
    fn vin(&self, vin: &str, lang: &Lang) -> Vec<VinMatch>;
    fn roots(&self, ctx: &Context, lang: &Lang) -> Vec<Node>;
    fn children(&self, node: i64, ctx: &Context, lang: &Lang) -> Vec<Node>;
    fn doc(&self, id: i64, lang: &Lang) -> Option<Doc>;
    /// mime, whether the bytes are gzip, bytes
    fn image(&self, id: i64) -> Option<(String, bool, Vec<u8>)>;
    fn static_file(&self, name: &str) -> Option<(String, Vec<u8>)>;
    /// A link whose target depends on the vehicle.
    fn cell(&self, targets: &str, ctx: &Context, lang: &Lang) -> Option<i64>;
    fn search(&self, q: &str, ctx: &Context, lang: &Lang, limit: usize) -> Vec<Hit>;
}

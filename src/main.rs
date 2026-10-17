//! openmanuals: workshop manuals by VIN or model, for a phone or a PC.
//!
//!     openmanuals [--packs DIR] [--listen ADDR]
//!
//! Every `*.sqlite` in the packs folder whose `meta.source` is known becomes a
//! source; the web app is built in.

mod globaltis;
mod pack;
mod tree;
mod vcr;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use pack::{Lang, Pack};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use vcr::Context;

#[derive(rust_embed::RustEmbed)]
#[folder = "web/"]
struct Web;

struct Source {
    title: String,
    release: String,
    pack: Arc<dyn Pack>,
}

type Sources = Arc<BTreeMap<String, Source>>;

#[derive(Deserialize, Default)]
struct Q {
    #[serde(default)]
    ctx: String,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    q: String,
    #[serde(default)]
    c: String,
}

impl Q {
    fn lang(&self) -> Lang {
        Lang(self.lang.clone().filter(|l| l.len() == 2).unwrap_or_else(|| "es".into()))
    }
    fn ctx(&self) -> Context {
        Context::parse(&self.ctx)
    }
}

fn open_packs(dir: &std::path::Path) -> BTreeMap<String, Source> {
    let mut out = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        eprintln!("no packs folder at {}", dir.display());
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("sqlite") {
            continue;
        }
        let id = p.file_stem().unwrap().to_string_lossy().to_string();
        let meta = |c: &rusqlite::Connection, k: &str| -> String {
            c.query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| r.get(0)).optional().ok().flatten().unwrap_or_default()
        };
        let Ok(c) = rusqlite::Connection::open_with_flags(&p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else { continue };
        let (kind, title, release) = (meta(&c, "source"), meta(&c, "title"), meta(&c, "release"));
        drop(c);
        let pack: Arc<dyn Pack> = match kind.as_str() {
            "globaltis" => match globaltis::GlobalTis::open(&p) {
                Ok(g) => Arc::new(g),
                Err(e) => {
                    eprintln!("{}: {e}", p.display());
                    continue;
                }
            },
            "tree" => match tree::Tree::open(&p) {
                Ok(t) => Arc::new(t),
                Err(e) => {
                    eprintln!("{}: {e}", p.display());
                    continue;
                }
            },
            other => {
                eprintln!("{}: unknown source kind {other:?}", p.display());
                continue;
            }
        };
        eprintln!("source {id}: {title} ({release})");
        out.insert(id, Source { title, release, pack });
    }
    out
}

#[tokio::main]
async fn main() {
    let mut packs = PathBuf::from("packs");
    let mut listen = "0.0.0.0:8460".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--packs" => packs = args.next().expect("--packs DIR").into(),
            "--listen" => listen = args.next().expect("--listen ADDR"),
            _ => {
                eprintln!("usage: openmanuals [--packs DIR] [--listen ADDR]");
                std::process::exit(2);
            }
        }
    }
    let sources: Sources = Arc::new(open_packs(&packs));
    let app = Router::new()
        .route("/api/sources", get(list_sources))
        .route("/api/vin/{vin}", get(vin))
        .route("/api/s/{src}/related", get(related))
        .route("/api/s/{src}/options", get(options))
        .route("/api/s/{src}/describe", get(describe))
        .route("/api/s/{src}/roots", get(roots))
        .route("/api/s/{src}/node/{id}", get(children))
        .route("/api/s/{src}/search", get(search))
        .route("/api/s/{src}/doc/{id}", get(doc))
        .route("/api/s/{src}/img/{id}", get(image))
        .route("/api/s/{src}/static/{name}", get(static_file))
        .route("/api/s/{src}/cell", get(cell))
        .fallback(get(web))
        .with_state(sources);
    let l = tokio::net::TcpListener::bind(&listen).await.expect("listen");
    eprintln!("openmanuals on http://{listen}");
    axum::serve(l, app).await.unwrap();
}

#[derive(Serialize)]
struct SourceInfo {
    id: String,
    title: String,
    release: String,
}

async fn list_sources(State(s): State<Sources>) -> Json<Vec<SourceInfo>> {
    Json(
        s.iter()
            .map(|(id, src)| SourceInfo { id: id.clone(), title: src.title.clone(), release: src.release.clone() })
            .collect(),
    )
}

fn pack(s: &Sources, src: &str) -> Result<Arc<dyn Pack>, StatusCode> {
    s.get(src).map(|x| x.pack.clone()).ok_or(StatusCode::NOT_FOUND)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.expect("worker")
}

#[derive(Serialize)]
struct VinAnswer {
    source: String,
    label: String,
    ctx: String,
}

async fn vin(State(s): State<Sources>, Path(v): Path<String>, Query(q): Query<Q>) -> Json<Vec<VinAnswer>> {
    let lang = q.lang();
    Json(
        blocking(move || {
            let mut out = Vec::new();
            for (id, src) in s.iter() {
                for m in src.pack.vin(&v, &lang) {
                    out.push(VinAnswer { source: id.clone(), label: m.label, ctx: m.ctx.to_string() });
                }
            }
            out
        })
        .await,
    )
}

/// The same vehicle in the other sources.
async fn related(State(s): State<Sources>, Path(src): Path<String>, Query(q): Query<Q>) -> Result<impl IntoResponse, StatusCode> {
    let p = pack(&s, &src)?;
    let lang = q.lang();
    Ok(Json(
        blocking(move || {
            let keys = p.keys(&q.ctx(), &lang);
            let mut out = Vec::new();
            for (id, other) in s.iter().filter(|(id, _)| **id != src) {
                for m in other.pack.find(&keys, &lang) {
                    out.push(VinAnswer { source: id.clone(), label: m.label, ctx: m.ctx.to_string() });
                }
            }
            out
        })
        .await,
    ))
}

async fn options(State(s): State<Sources>, Path(src): Path<String>, Query(q): Query<Q>) -> Result<impl IntoResponse, StatusCode> {
    let p = pack(&s, &src)?;
    Ok(Json(blocking(move || p.options(&q.ctx(), &q.lang())).await))
}

async fn describe(State(s): State<Sources>, Path(src): Path<String>, Query(q): Query<Q>) -> Result<impl IntoResponse, StatusCode> {
    let p = pack(&s, &src)?;
    Ok(Json(blocking(move || p.describe(&q.ctx(), &q.lang())).await))
}

async fn roots(State(s): State<Sources>, Path(src): Path<String>, Query(q): Query<Q>) -> Result<impl IntoResponse, StatusCode> {
    let p = pack(&s, &src)?;
    Ok(Json(blocking(move || p.roots(&q.ctx(), &q.lang())).await))
}

async fn children(
    State(s): State<Sources>,
    Path((src, id)): Path<(String, i64)>,
    Query(q): Query<Q>,
) -> Result<impl IntoResponse, StatusCode> {
    let p = pack(&s, &src)?;
    Ok(Json(blocking(move || p.children(id, &q.ctx(), &q.lang())).await))
}

async fn search(State(s): State<Sources>, Path(src): Path<String>, Query(q): Query<Q>) -> Result<impl IntoResponse, StatusCode> {
    let p = pack(&s, &src)?;
    Ok(Json(blocking(move || p.search(&q.q, &q.ctx(), &q.lang(), 200)).await))
}

fn cached(mut r: Response, secs: u32) -> Response {
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_str(&format!("public, max-age={secs}")).unwrap());
    r
}

/// The document as stored, plus the reader script; links in it are relative to
/// the source, so a `<base>` makes them resolve.
async fn doc(State(s): State<Sources>, Path((src, id)): Path<(String, i64)>, Query(q): Query<Q>) -> Result<Response, StatusCode> {
    let p = pack(&s, &src)?;
    let lang = q.lang();
    let d = blocking(move || p.doc(id, &lang)).await.ok_or(StatusCode::NOT_FOUND)?;
    let mut html = String::new();
    {
        use std::io::Read;
        flate2::read::GzDecoder::new(&d.gzip_html[..]).read_to_string(&mut html).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let mut inject = format!(
        "<base href=\"/api/s/{src}/\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <link rel=\"stylesheet\" href=\"/reader.css\"><script src=\"/reader.js\" defer></script>"
    );
    if !html.contains("<title") && !html.contains("<TITLE") {
        inject.push_str(&format!("<title>{}</title>", d.title.replace('<', "&lt;")));
    }
    let html = match html.find("<head>").or_else(|| html.find("<HEAD>")) {
        Some(i) => format!("{}{}{}", &html[..i + 6], inject, &html[i + 6..]),
        None => format!("<!doctype html><head>{inject}</head>{html}"),
    };
    let mut r = Response::new(Body::from(html));
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    Ok(cached(r, 3600))
}

async fn image(State(s): State<Sources>, Path((src, id)): Path<(String, i64)>) -> Result<Response, StatusCode> {
    let p = pack(&s, &src)?;
    let (mime, gz, data) = blocking(move || p.image(id)).await.ok_or(StatusCode::NOT_FOUND)?;
    let mut r = Response::new(Body::from(data));
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap_or(HeaderValue::from_static("application/octet-stream")));
    if gz {
        r.headers_mut().insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    }
    Ok(cached(r, 86400 * 30))
}

async fn static_file(State(s): State<Sources>, Path((src, name)): Path<(String, String)>) -> Result<Response, StatusCode> {
    let p = pack(&s, &src)?;
    let (mime, data) = blocking(move || p.static_file(&name)).await.ok_or(StatusCode::NOT_FOUND)?;
    let mut r = Response::new(Body::from(data));
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap());
    Ok(cached(r, 86400))
}

async fn cell(State(s): State<Sources>, Path(src): Path<String>, Query(q): Query<Q>) -> Result<Response, StatusCode> {
    let p = pack(&s, &src)?;
    let lang = q.lang();
    let ctx = q.ctx();
    let c = q.c.clone();
    let id = blocking(move || p.cell(&c, &ctx, &lang)).await.ok_or(StatusCode::NOT_FOUND)?;
    Ok(Redirect::temporary(&format!("/api/s/{src}/doc/{id}?lang={}&ctx={}", q.lang().0, q.ctx)).into_response())
}

async fn web(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() || Web::get(path).is_none() { "index.html" } else { path };
    match Web::get(path) {
        Some(f) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let mut r = Response::new(Body::from(f.data.into_owned()));
            r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap());
            r
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

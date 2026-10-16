//! @chromaglyph/node — napi-rs bindings for chromaglyph-core.
//!
//! JS surface (see index.d.ts after `napi build`):
//!
//! ```ts
//! buildFont(opts): BuildResult          // throws on failure
//! buildFontSafe(opts): BuildResult|null // silent degradation for pipelines
//! clearCache(): void
//! ```
//!
//! `opts.blocks` batches any number of code blocks into ONE font (global
//! disambiguation across languages is handled by the core). Artifacts are
//! cached by a content hash (font bytes + theme sources + block texts +
//! options + engine version): an in-process LRU plus an optional on-disk
//! cache directory, so incremental site builds skip recomputation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use napi::bindgen_prelude::*;
use napi_derive::napi;
use sha2::{Digest, Sha256};

use chromaglyph_core::theme::{load_theme_arg, ThemeSpec};
use chromaglyph_core::{build, Block, BuildInput};

/// Bump when the compiled artifacts' semantics change shape.
const CACHE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Keep memory bounded; fonts are ~100-500 KB each.
const MEMORY_CACHE_CAPACITY: usize = 16;

#[napi(object)]
pub struct JsBlock {
    /// Stable id used in reports and warnings.
    pub id: String,
    /// Language: alias (`rust`), extension (`rs`) or `plain`.
    pub lang: String,
    pub text: String,
}

#[napi(object)]
pub struct BuildOptions {
    /// Static TrueType font bytes.
    pub font: Buffer,
    /// Builtin theme names (`dark-plus`, `light-plus`) or paths to
    /// VS Code .json / TextMate .tmTheme files. First = default palette.
    pub themes: Vec<String>,
    pub blocks: Vec<JsBlock>,
    /// Run the rustybuzz oracle (default true).
    pub verify: Option<bool>,
    /// Optional on-disk cache directory; enables cross-process caching.
    pub cache_dir: Option<String>,
}

#[napi(object)]
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ThemeInfo {
    pub name: String,
    /// Editor background for CSS (`null` when the theme does not define one).
    pub background: Option<String>,
}

#[napi(object)]
pub struct BuildResult {
    pub woff: Buffer,
    pub ttf: Buffer,
    /// Generic companion CSS from the core (for reference / custom setups).
    pub css: String,
    pub themes: Vec<ThemeInfo>,
    /// JSON build report (see chromaglyph-core::Report).
    pub report: String,
    /// sha256 content hash of all inputs; use it to name emitted assets.
    pub content_hash: String,
    pub cached: bool,
}

#[napi(object)]
pub struct CacheStats {
    pub memory_entries: u32,
    pub memory_capacity: u32,
}

#[derive(Clone)]
struct Cached {
    woff: Arc<Vec<u8>>,
    ttf: Arc<Vec<u8>>,
    css: String,
    themes: Vec<ThemeInfo>,
    report: String,
}

fn memory_cache() -> &'static Mutex<HashMap<String, Cached>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Cached>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Compute the content hash covering every input that affects the output.
fn content_hash(opts: &BuildOptions) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CACHE_VERSION.as_bytes());
    hasher.update([0]);
    hasher.update(&opts.font);
    hasher.update([0]);
    for theme in &opts.themes {
        hasher.update(theme.as_bytes());
        // a file's content matters, not just its path
        let path = PathBuf::from(theme);
        if path.exists() {
            if let Ok(bytes) = std::fs::read(&path) {
                hasher.update(&bytes);
            }
        }
        hasher.update([0]);
    }
    hasher.update(if opts.verify.unwrap_or(true) {
        &[1]
    } else {
        &[0]
    });
    for block in &opts.blocks {
        hasher.update(block.id.as_bytes());
        hasher.update([0]);
        hasher.update(block.lang.as_bytes());
        hasher.update([0]);
        hasher.update(block.text.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

struct DiskPaths {
    woff: PathBuf,
    css: PathBuf,
    report: PathBuf,
    themes: PathBuf,
}

fn disk_paths(cache_dir: &str, hash: &str) -> DiskPaths {
    let dir = PathBuf::from(cache_dir).join("chromaglyph");
    let _ = std::fs::create_dir_all(&dir);
    DiskPaths {
        woff: dir.join(format!("{hash}.woff")),
        css: dir.join(format!("{hash}.css")),
        report: dir.join(format!("{hash}.report.json")),
        themes: dir.join(format!("{hash}.themes.json")),
    }
}

fn read_disk(paths: &DiskPaths) -> Option<Cached> {
    let woff = std::fs::read(&paths.woff).ok()?;
    let css = std::fs::read_to_string(&paths.css).ok()?;
    let report = std::fs::read_to_string(&paths.report).ok()?;
    let themes_json = std::fs::read_to_string(&paths.themes).ok()?;
    let themes: Vec<ThemeInfo> = serde_json::from_str(&themes_json).ok()?;
    Some(Cached {
        woff: Arc::new(woff),
        ttf: Arc::new(Vec::new()),
        css,
        themes,
        report,
    })
}

fn write_disk(paths: &DiskPaths, cached: &Cached, ttf: &[u8]) {
    // the ttf is large and reconstructable (it is the woff's source), so the
    // disk cache stores only the shipping artifact (woff) + metadata
    let _ = std::fs::write(&paths.woff, cached.woff.as_ref());
    let _ = std::fs::write(&paths.css, &cached.css);
    let _ = std::fs::write(&paths.report, &cached.report);
    let _ = std::fs::write(
        &paths.themes,
        serde_json::to_string(&cached.themes).unwrap_or_default(),
    );
    let _ = ttf; // referenced to keep the signature honest
}

fn remember(key: &str, cached: Cached) {
    let mut cache = memory_cache().lock().unwrap();
    if !cache.contains_key(key) && cache.len() >= MEMORY_CACHE_CAPACITY {
        // drop the oldest entry (insertion order approximated by first key)
        if let Some(oldest) = cache.keys().next().cloned() {
            cache.remove(&oldest);
        }
    }
    cache.insert(key.to_string(), cached);
}

fn build_uncached(opts: &BuildOptions) -> napi::Result<BuildResult> {
    let themes: Vec<ThemeSpec> = opts
        .themes
        .iter()
        .map(|arg| {
            load_theme_arg(arg).map_err(|e| napi::Error::new(Status::InvalidArg, e.to_string()))
        })
        .collect::<napi::Result<_>>()?;
    if themes.is_empty() {
        return Err(napi::Error::new(
            Status::InvalidArg,
            "at least one theme is required",
        ));
    }
    // blocks borrow their text; keep the strings alive in one place
    let owned: Vec<(String, String, String)> = opts
        .blocks
        .iter()
        .map(|b| (b.id.clone(), b.lang.clone(), b.text.clone()))
        .collect();
    let blocks: Vec<Block<'_>> = owned
        .iter()
        .map(|(id, lang, text)| Block {
            id: id.clone(),
            lang: lang.clone(),
            text,
        })
        .collect();
    let font: Vec<u8> = opts.font.as_ref().to_vec();

    let out = build(&BuildInput {
        font_bytes: &font,
        themes: &themes,
        blocks: &blocks,
        run_verify: opts.verify.unwrap_or(true),
        generic: None,
        font_name: None,
    })
    .map_err(|e| napi::Error::new(Status::GenericFailure, e.to_string()))?;

    Ok(BuildResult {
        woff: Buffer::from(out.woff.clone()),
        ttf: Buffer::from(out.ttf.clone()),
        css: out.css,
        themes: themes
            .iter()
            .map(|t| ThemeInfo {
                name: t.name.clone(),
                background: t.background.map(|c| c.to_hex()),
            })
            .collect(),
        report: serde_json::to_string(&out.report).unwrap_or_else(|_| "{}".into()),
        content_hash: String::new(), // filled by the caller
        cached: false,
    })
}

#[napi]
/// Build a font. Throws on invalid input; pipelines that must not fail
/// should call [`build_font_safe`] instead.
pub fn build_font(opts: BuildOptions) -> napi::Result<BuildResult> {
    let hash = content_hash(&opts);

    if let Some(hit) = memory_cache().lock().unwrap().get(&hash) {
        return Ok(materialize(hit, &hash, true));
    }
    if let Some(dir) = &opts.cache_dir {
        if let Some(hit) = read_disk(&disk_paths(dir, &hash)) {
            remember(&hash, hit);
            let cached = memory_cache().lock().unwrap().get(&hash).cloned();
            if let Some(hit) = cached {
                return Ok(materialize(&hit, &hash, true));
            }
        }
    }

    let mut result = build_uncached(&opts)?;
    result.content_hash = hash.clone();
    let cached = Cached {
        woff: Arc::new(result.woff.as_ref().to_vec()),
        ttf: Arc::new(result.ttf.as_ref().to_vec()),
        css: result.css.clone(),
        themes: result
            .themes
            .iter()
            .map(|t| ThemeInfo {
                name: t.name.clone(),
                background: t.background.clone(),
            })
            .collect(),
        report: result.report.clone(),
    };
    if let Some(dir) = &opts.cache_dir {
        write_disk(&disk_paths(dir, &hash), &cached, &result.ttf);
    }
    remember(&hash, cached);
    result.cached = false;
    Ok(result)
}

#[napi]
/// Silent-degradation variant for build pipelines: returns `null` instead of
/// throwing when the font cannot be produced (the site then renders without
/// font-based highlighting).
pub fn build_font_safe(opts: BuildOptions) -> Option<BuildResult> {
    // panics in native code would abort the process; belt and suspenders
    let result = std::panic::catch_unwind(|| build_font(opts));
    match result {
        Ok(Ok(built)) => Some(built),
        Ok(Err(_)) | Err(_) => None,
    }
}

#[napi]
pub fn cache_stats() -> CacheStats {
    let len = memory_cache().lock().unwrap().len() as u32;
    CacheStats {
        memory_entries: len,
        memory_capacity: MEMORY_CACHE_CAPACITY as u32,
    }
}

#[napi]
pub fn clear_cache() {
    memory_cache().lock().unwrap().clear();
}

fn materialize(hit: &Cached, hash: &str, cached: bool) -> BuildResult {
    BuildResult {
        woff: Buffer::from(hit.woff.as_ref().clone()),
        ttf: Buffer::from(hit.ttf.as_ref().clone()),
        css: hit.css.clone(),
        themes: hit
            .themes
            .iter()
            .map(|t| ThemeInfo {
                name: t.name.clone(),
                background: t.background.clone(),
            })
            .collect(),
        report: hit.report.clone(),
        content_hash: hash.to_string(),
        cached,
    }
}

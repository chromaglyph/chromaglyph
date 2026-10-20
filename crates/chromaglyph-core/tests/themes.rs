//! Every vendored builtin theme loads and highlights. Iterates the registry,
//! so a re-vendor (`scripts/vendor-themes.mjs`) is covered without edits.

use std::path::Path;

use chromaglyph_core::theme::{
    builtin_theme, builtin_themes, ThemeKind, ThemeSpec, TM_THEMES_VERSION,
};
use chromaglyph_core::Color;

fn crate_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// WCAG contrast ratio (1:1 identical … 21:1 black on white).
fn contrast_ratio(a: Color, b: Color) -> f64 {
    fn luminance(c: Color) -> f64 {
        let channel = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.red) + 0.7152 * channel(c.green) + 0.0722 * channel(c.blue)
    }
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[test]
fn every_builtin_loads_and_colors_code() {
    let mut problems = Vec::new();
    for t in builtin_themes() {
        let spec = match ThemeSpec::builtin(t.slug) {
            Ok(spec) => spec,
            Err(e) => {
                problems.push(format!("{}: {e}", t.slug));
                continue;
            }
        };
        if spec.name != t.slug {
            problems.push(format!("{}: name is {:?}", t.slug, spec.name));
        }
        let Some(bg) = spec.background else {
            problems.push(format!("{}: no editor background", t.slug));
            continue;
        };
        // the registry's dark/light must agree with what the theme looks
        // like, and the default text must be readable on the background —
        // this is what the default-foreground fallback exists for
        let bg_matches_kind = match t.kind {
            ThemeKind::Dark => !bg.is_light(),
            ThemeKind::Light => bg.is_light(),
        };
        if !bg_matches_kind {
            problems.push(format!(
                "{}: declared {} but background {} is {}",
                t.slug,
                t.kind.as_str(),
                bg.to_hex(),
                if bg.is_light() { "light" } else { "dark" }
            ));
        }
        // the default text must be readable on the background — this is
        // what the default-foreground fallback exists for. 1.5:1 is far
        // below any accessibility bar (material-theme-lighter ships 2.5:1
        // on purpose) but catches a dark default landing on a light theme
        let ratio = contrast_ratio(spec.foreground, bg);
        if ratio < 1.5 {
            problems.push(format!(
                "{}: foreground {} on background {} has contrast {ratio:.2}:1",
                t.slug,
                spec.foreground.to_hex(),
                bg.to_hex()
            ));
        }
        let keyword = spec.scope_color("keyword.control");
        let string = spec.scope_color("string.quoted.double");
        if keyword.is_none() && string.is_none() {
            problems.push(format!(
                "{}: neither keywords nor strings are colored",
                t.slug
            ));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn registry_mirrors_index_json() {
    let index: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(crate_dir().join("themes/tm/index.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(index["tm-themes"], TM_THEMES_VERSION);
    let entries = index["themes"].as_array().unwrap();
    assert_eq!(entries.len(), builtin_themes().len());
    for (entry, t) in entries.iter().zip(builtin_themes()) {
        assert_eq!(entry["name"], t.slug);
        assert_eq!(entry["displayName"], t.display_name);
        assert_eq!(entry["type"], t.kind.as_str());
        assert_eq!(entry["license"], t.license);
        assert_eq!(entry["source"], t.source);
        assert!(
            crate_dir()
                .join(format!("themes/tm/{}.json", t.slug))
                .is_file(),
            "{}: vendored file missing",
            t.slug
        );
    }
}

#[test]
fn vendored_version_is_the_pinned_dependency() {
    // `pnpm up tm-themes` without re-running the vendor script would leave
    // the lockfile and the embedded collection disagreeing
    let root: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(crate_dir().join("../../package.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(root["devDependencies"]["tm-themes"], TM_THEMES_VERSION);
}

#[test]
fn only_permissive_licenses_ship() {
    for t in builtin_themes() {
        assert!(
            matches!(t.license, "MIT" | "Apache-2.0"),
            "{}: {} is not in the allowlist",
            t.slug,
            t.license
        );
        assert!(!t.license_url.is_empty(), "{}: no license URL", t.slug);
    }
    assert!(
        builtin_theme("aurora-x").is_none(),
        "GPL-3.0 theme must not ship"
    );
}

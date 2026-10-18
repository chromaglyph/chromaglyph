//! chromaglyph CLI: `build` and `verify`.
//!
//!   chromaglyph build --font JetBrainsMono-Regular.ttf --lang rust \
//!       --input code.rs --output out.woff \
//!       [--theme dark-plus --theme light-plus | --theme my-theme.json ...] \
//!       [--css snippet.css] [--report r.json] [--no-verify]
//!   chromaglyph verify --font out.woff --input code.rs [--lang rust] [--theme ...]
//!
//! The output container follows --output's extension: .ttf raw sfnt,
//! .woff2 WOFF2, anything else WOFF1.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chromaglyph_core::theme::load_theme_arg;
use chromaglyph_core::{build, verify, Block, BuildError, BuildInput, ThemeSpec};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "chromaglyph",
    version,
    about = "Compile syntax highlighting into a font"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build a color font from a source font and known content.
    Build {
        /// Source font: static TTF (TrueType) or OTF (CFF, name-keyed);
        /// WOFF1/WOFF2 containers and .ttc collections are unwrapped,
        /// variable TrueType fonts are pinned to their default instance
        #[arg(long)]
        font: PathBuf,
        /// Font to use inside a .ttc collection (0-based; ignored for
        /// single fonts)
        #[arg(long, default_value = "0")]
        font_index: u32,
        /// Input source file whose content is compiled into the font
        /// (exactly one of --input / --generic)
        #[arg(long, conflicts_with = "generic")]
        input: Option<PathBuf>,
        /// Language of the input: `auto` (detect from --input's extension),
        /// a name (`rust`, `typescript`, …) or an extension (`rs`)
        #[arg(long, default_value = "auto")]
        lang: String,
        /// Generic mode: build a font that highlights ANY text of this
        /// language (keywords, comments, strings, numbers) instead of
        /// compiling --input; one of `rust`, `js`, `ts`, `python`, `go`,
        /// `java`, `c`, `cpp`, `csharp`, `php`, `ruby`, `lua`, `shell`,
        /// `html`, `css`
        #[arg(long, conflicts_with = "input")]
        generic: Option<String>,
        /// Theme: builtin name (`dark-plus`, `light-plus`) or a path to a
        /// VS Code .json / TextMate .tmTheme file. Repeat for multiple
        /// palettes; the first is the default.
        #[arg(long = "theme", default_values_t = default_themes())]
        themes: Vec<String>,
        /// Output font path; .ttf writes raw sfnt, .woff2 wraps WOFF2
        /// (Brotli; both outline flavors), any other extension wraps
        /// WOFF1
        #[arg(long, default_value = "chromaglyph.woff")]
        output: PathBuf,
        /// Optional path for the companion CSS (font-face + palettes)
        #[arg(long)]
        css: Option<PathBuf>,
        /// Optional path for the JSON build report
        #[arg(long)]
        report: Option<PathBuf>,
        /// Skip the built-in rustybuzz verification pass
        #[arg(long)]
        no_verify: bool,
        /// Overlay tree-sitter semantic colors (parameters, types, calls,
        /// constants) on top of the syntactic highlighting; requires a
        /// language tree-sitter knows (currently rust, js/ts, python, java)
        #[arg(long)]
        semantic: bool,
        /// Preserve the source font's ligatures (`!=`, `->`, `www`, …) in
        /// exact mode: the build shapes the input with the source font and
        /// re-materializes each substitution as a rule, so ligatures render
        /// exactly as the font intends while staying highlighted. Sites
        /// spanning multiple colors degrade to plain per-char text
        /// (colors win). Ignored with --generic.
        #[arg(long)]
        ligatures: bool,
    },
    /// Re-check a built font against input content (the rustybuzz oracle).
    Verify {
        /// Font produced by `chromaglyph build` (.ttf, .woff or .woff2)
        #[arg(long)]
        font: PathBuf,
        /// The same source file that was built with
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value = "auto")]
        lang: String,
        #[arg(long = "theme", default_values_t = default_themes())]
        themes: Vec<String>,
    },
}

fn default_themes() -> Vec<String> {
    vec![String::from("dark-plus"), String::from("light-plus")]
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Build {
            font,
            font_index,
            input,
            lang,
            generic,
            themes,
            output,
            css,
            report,
            no_verify,
            semantic,
            ligatures,
        } => cmd_build(
            &font,
            font_index,
            input.as_deref(),
            &lang,
            generic.as_deref(),
            &themes,
            &output,
            css.as_deref(),
            report.as_deref(),
            no_verify,
            semantic,
            ligatures,
        ),
        Command::Verify {
            font,
            input,
            lang,
            themes,
        } => cmd_verify(&font, &input, &lang, &themes),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn read_required(path: &Path) -> Result<Vec<u8>, BuildError> {
    fs::read(path).map_err(|e| BuildError::Io(format!("cannot read {}: {e}", path.display())))
}

/// Resolve `--lang auto` against the input file's extension.
fn resolve_lang(lang: &str, input: &Path) -> String {
    if lang != "auto" {
        return lang.to_string();
    }
    input
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "plain".into())
}

fn one_block<'a>(id: &str, lang: &str, bytes: &'a [u8]) -> Result<Block<'a>, BuildError> {
    // A block's text must stay byte-identical to what will sit in the DOM;
    // we do not normalize line endings.
    let text = std::str::from_utf8(bytes)
        .map_err(|e| BuildError::Io(format!("input is not valid UTF-8: {e}")))?;
    Ok(Block {
        id: id.to_string(),
        lang: lang.to_string(),
        text,
    })
}

fn load_themes(args: &[String]) -> Result<Vec<ThemeSpec>, BuildError> {
    let mut themes = Vec::with_capacity(args.len());
    for arg in args {
        let spec =
            load_theme_arg(arg).map_err(|e| BuildError::Theme(format!("--theme {arg:?}: {e}")))?;
        println!("theme: {} ({})", spec.name, arg);
        themes.push(spec);
    }
    Ok(themes)
}

#[allow(clippy::too_many_arguments)]
fn cmd_build(
    font_path: &Path,
    font_index: u32,
    input_path: Option<&Path>,
    lang: &str,
    generic: Option<&str>,
    theme_args: &[String],
    output: &Path,
    css_path: Option<&Path>,
    report_path: Option<&Path>,
    no_verify: bool,
    semantic: bool,
    ligatures: bool,
) -> Result<(), BuildError> {
    let font_bytes = read_required(font_path)?;
    let generic_spec = match (generic, input_path) {
        (Some(name), _) => {
            println!("generic mode: {name}");
            Some(chromaglyph_core::generic::spec_by_name(name)?)
        }
        (None, Some(_)) => None,
        (None, None) => {
            return Err(BuildError::Io(
                "build needs either --input or --generic".into(),
            ))
        }
    };
    let input_bytes;
    let block: Option<Block<'_>> = match input_path {
        Some(path) => {
            input_bytes = read_required(path)?;
            let lang = resolve_lang(lang, path);
            Some(one_block("main", &lang, &input_bytes)?)
        }
        None => None,
    };
    let themes = load_themes(theme_args)?;

    let semantic_cb = |block: &Block<'_>| -> Vec<(usize, usize, String)> {
        chromaglyph_treesitter::semantic_spans(&block.lang, block.text)
            .into_iter()
            .map(|s| (s.start, s.end, s.scope.to_string()))
            .collect()
    };

    let format = match output.extension().and_then(|e| e.to_str()) {
        Some("ttf") => chromaglyph_core::OutputFormat::Sfnt,
        Some("woff2") => chromaglyph_core::OutputFormat::Woff2,
        _ => chromaglyph_core::OutputFormat::Woff,
    };
    let out = build(&BuildInput {
        font_bytes: &font_bytes,
        font_index,
        themes: &themes,
        blocks: block.as_slice(),
        run_verify: !no_verify,
        generic: generic_spec.as_ref(),
        font_name: output
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_owned)
            .as_deref(),
        semantic: semantic.then_some(&semantic_cb as chromaglyph_core::SemanticLayer),
        ligatures: ligatures && generic_spec.is_none(),
        format,
    })?;

    let is_ttf = output.extension().map(|e| e == "ttf").unwrap_or(false);
    let font_bytes_out = if is_ttf { &out.ttf } else { &out.woff };
    fs::write(output, font_bytes_out)
        .map_err(|e| BuildError::Io(format!("cannot write {}: {e}", output.display())))?;

    if let Some(path) = css_path {
        fs::write(path, out.css)
            .map_err(|e| BuildError::Io(format!("cannot write {}: {e}", path.display())))?;
    }
    let report_json = serde_json::to_string_pretty(&out.report).unwrap();
    if let Some(path) = report_path {
        fs::write(path, &report_json)
            .map_err(|e| BuildError::Io(format!("cannot write {}: {e}", path.display())))?;
    }

    let r = &out.report;
    println!(
        "built {} ({} bytes; ttf {} bytes)",
        output.display(),
        font_bytes_out.len(),
        out.ttf.len()
    );
    println!(
        "  {} blocks, {} chars ({} colored), {} slots, {} rules, {} colored glyph copies",
        r.blocks, r.chars, r.colored_cells, r.slots, r.rules, r.alt_glyphs
    );
    if r.lig_rules > 0 {
        println!(
            "  ligatures: {} rule(s) over {} site(s) restored from the source font's shaping",
            r.lig_rules, r.lig_sites
        );
    }
    println!(
        "  verify: {} ({} glyphs, {} colored, {} palettes)",
        if r.verify.ok { "OK" } else { "FAILED" },
        r.verify.glyphs_checked,
        r.verify.colored_glyphs,
        r.verify.palettes_checked
    );
    for warning in &r.warnings {
        println!("  warning: {warning}");
    }
    if let Some(path) = report_path {
        println!("  report: {}", path.display());
    }
    Ok(())
}

fn cmd_verify(
    font_path: &Path,
    input_path: &Path,
    lang: &str,
    theme_args: &[String],
) -> Result<(), BuildError> {
    let font_bytes = read_required(font_path)?;
    let input_bytes = read_required(input_path)?;
    let lang = resolve_lang(lang, input_path);
    let block = one_block("main", &lang, &input_bytes)?;
    let themes = load_themes(theme_args)?;
    let result = verify(&font_bytes, &[block], &themes)?;
    if result.ok {
        println!(
            "verify OK: {} glyphs checked, {} colored, {} palettes, 0 mismatches",
            result.glyphs_checked, result.colored_glyphs, result.palettes_checked
        );
        Ok(())
    } else {
        Err(BuildError::VerifyFailed(result.failures))
    }
}

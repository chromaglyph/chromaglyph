//! chromaglyph CLI: `build` and `verify`.
//!
//!   chromaglyph build --font JetBrainsMono-Regular.ttf --lang rust \
//!       --input code.rs --output out.woff \
//!       [--theme dark-plus --theme light-plus | --theme my-theme.json ...] \
//!       [--css snippet.css] [--report r.json] [--no-verify]
//!   chromaglyph verify --font out.woff --input code.rs [--lang rust] [--theme ...]

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
    /// Build a color font from a static TTF and known content.
    Build {
        /// Source font: static TrueType (glyf) outlines, e.g. JetBrainsMono-Regular.ttf
        #[arg(long)]
        font: PathBuf,
        /// Input source file whose content is compiled into the font
        #[arg(long)]
        input: PathBuf,
        /// Language of the input: `auto` (detect from --input's extension),
        /// a name (`rust`, `typescript`, …) or an extension (`rs`)
        #[arg(long, default_value = "auto")]
        lang: String,
        /// Theme: builtin name (`dark-plus`, `light-plus`) or a path to a
        /// VS Code .json / TextMate .tmTheme file. Repeat for multiple
        /// palettes; the first is the default.
        #[arg(long = "theme", default_values_t = default_themes())]
        themes: Vec<String>,
        /// Output font path; .ttf writes raw sfnt, .woff (default) wraps WOFF1
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
    },
    /// Re-check a built font against input content (the rustybuzz oracle).
    Verify {
        /// Font produced by `chromaglyph build` (.ttf or .woff)
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
            input,
            lang,
            themes,
            output,
            css,
            report,
            no_verify,
        } => cmd_build(
            &font,
            &input,
            &lang,
            &themes,
            &output,
            css.as_deref(),
            report.as_deref(),
            no_verify,
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
    input_path: &Path,
    lang: &str,
    theme_args: &[String],
    output: &Path,
    css_path: Option<&Path>,
    report_path: Option<&Path>,
    no_verify: bool,
) -> Result<(), BuildError> {
    let font_bytes = read_required(font_path)?;
    let input_bytes = read_required(input_path)?;
    let lang = resolve_lang(lang, input_path);
    let block = one_block("main", &lang, &input_bytes)?;
    let themes = load_themes(theme_args)?;

    let out = build(&BuildInput {
        font_bytes: &font_bytes,
        themes: &themes,
        blocks: &[block],
        run_verify: !no_verify,
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

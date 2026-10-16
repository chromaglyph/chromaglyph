//! chromaglyph CLI: `build` and `verify`.
//!
//! M1 surface:
//!   chromaglyph build --font JetBrainsMono-Regular.ttf --lang rust \
//!       --input code.rs --output out.woff [--css snippet.css] [--report r.json]
//!       [--no-verify]
//!   chromaglyph verify --font out.woff --lang rust --input code.rs

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chromaglyph_core::{build, verify, Block, BuildError, BuildInput, Theme};
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
        /// Language of the input (rust in M1; unknown falls back to plain)
        #[arg(long, default_value = "rust")]
        lang: String,
        /// Output font path; .ttf writes raw sfnt, .woff (default) wraps WOFF1
        #[arg(long, default_value = "chromaglyph.woff")]
        output: PathBuf,
        /// Optional path for the companion CSS snippet
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
        #[arg(long, default_value = "rust")]
        lang: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Build {
            font,
            input,
            lang,
            output,
            css,
            report,
            no_verify,
        } => cmd_build(
            &font,
            &input,
            &lang,
            &output,
            css.as_deref(),
            report.as_deref(),
            no_verify,
        ),
        Command::Verify { font, input, lang } => cmd_verify(&font, &input, &lang),
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

fn cmd_build(
    font_path: &Path,
    input_path: &Path,
    lang: &str,
    output: &Path,
    css_path: Option<&Path>,
    report_path: Option<&Path>,
    no_verify: bool,
) -> Result<(), BuildError> {
    let font_bytes = read_required(font_path)?;
    let input_bytes = read_required(input_path)?;
    let block = one_block("main", lang, &input_bytes)?;
    let theme = Theme::builtin_dark();

    let out = build(&BuildInput {
        font_bytes: &font_bytes,
        theme: &theme,
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
        "  {} blocks, {} chars, {} tokens ({} uncolored) -> {} rules, {} colored glyph copies, palette {}",
        r.blocks,
        r.chars,
        r.tokens,
        r.uncolored_tokens,
        r.rules,
        r.alt_glyphs,
        r.palette.join(" ")
    );
    println!(
        "  verify: {} ({} glyphs, {} colored)",
        if r.verify.ok { "OK" } else { "FAILED" },
        r.verify.glyphs_checked,
        r.verify.colored_glyphs
    );
    for warning in &r.warnings {
        println!("  warning: {warning}");
    }
    if let Some(path) = report_path {
        println!("  report: {}", path.display());
    }
    Ok(())
}

fn cmd_verify(font_path: &Path, input_path: &Path, lang: &str) -> Result<(), BuildError> {
    let font_bytes = read_required(font_path)?;
    let input_bytes = read_required(input_path)?;
    let block = one_block("main", lang, &input_bytes)?;
    let theme = Theme::builtin_dark();
    let result = verify(&font_bytes, &[block], &theme)?;
    if result.ok {
        println!(
            "verify OK: {} glyphs checked, {} colored, 0 mismatches",
            result.glyphs_checked, result.colored_glyphs
        );
        Ok(())
    } else {
        Err(BuildError::VerifyFailed(result.failures))
    }
}

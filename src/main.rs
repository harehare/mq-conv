use std::fs;
use std::io::{self, BufWriter, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, ValueEnum};
use miette::IntoDiagnostic;

use mq_conv::budget;
use mq_conv::converter::ConvertOptions;
use mq_conv::detect::Format;
use mq_conv::formats::media::relative_path;
use mq_conv::parallel::par_map;

#[derive(Parser, Debug)]
#[command(name = "mq-conv")]
#[command(version, about = "Convert various file formats to Markdown")]
struct Args {
    /// Input file paths (reads from stdin if not provided)
    files: Vec<PathBuf>,

    /// Force a specific format instead of auto-detecting
    #[arg(short, long)]
    format: Option<FormatArg>,

    /// Output directory for individual output files (one per input file)
    #[arg(short, long)]
    output_dir: Option<PathBuf>,

    /// Target output format when converting from Markdown
    #[arg(long)]
    to: Option<ToArg>,

    /// Tesseract language for OCR, e.g. "jpn" or "eng+jpn" (requires the
    /// matching tesseract-ocr language pack to be installed)
    #[arg(long, default_value = "eng")]
    ocr_lang: String,

    /// Extract embedded images (PDF, DOCX, PPTX, EPUB) into this directory and
    /// reference them from the Markdown. With several inputs each file gets
    /// its own sub-directory.
    #[arg(long, value_name = "DIR")]
    extract_images: Option<PathBuf>,

    /// Emit at most roughly this many tokens of Markdown (cut at block
    /// boundaries). A trailing comment gives the cursor for the next chunk.
    /// Single input only.
    #[arg(long, value_name = "N")]
    max_tokens: Option<usize>,

    /// Resume output from this cursor (byte offset printed by a previous
    /// --max-tokens run). Single input only.
    #[arg(long, value_name = "OFFSET", default_value_t = 0)]
    cursor: usize,
}

#[derive(ValueEnum, Clone, Debug)]
enum FormatArg {
    Excel,
    Pdf,
    Powerpoint,
    Word,
    Image,
    Zip,
    Epub,
    Audio,
    Csv,
    Html,
    Json,
    Yaml,
    Toml,
    Xml,
    Sqlite,
    Tar,
    Video,
    Ocr,
    MarkdownDocx,
    Rst,
    Org,
    Mediawiki,
    Asciidoc,
}

#[derive(ValueEnum, Clone, Debug)]
enum ToArg {
    Html,
    Text,
    Latex,
    Rst,
    Asciidoc,
    Org,
    Epub,
    Json,
    Docx,
}

impl From<ToArg> for Format {
    fn from(arg: ToArg) -> Self {
        match arg {
            ToArg::Html => Format::MarkdownHtml,
            ToArg::Text => Format::MarkdownText,
            ToArg::Latex => Format::MarkdownLatex,
            ToArg::Rst => Format::MarkdownRst,
            ToArg::Asciidoc => Format::MarkdownAsciidoc,
            ToArg::Org => Format::MarkdownOrg,
            ToArg::Epub => Format::MarkdownEpub,
            ToArg::Json => Format::MarkdownJsonAst,
            ToArg::Docx => Format::MarkdownDocx,
        }
    }
}

impl From<FormatArg> for Format {
    fn from(arg: FormatArg) -> Self {
        match arg {
            FormatArg::Excel => Format::Excel,
            FormatArg::Pdf => Format::Pdf,
            FormatArg::Powerpoint => Format::PowerPoint,
            FormatArg::Word => Format::Word,
            FormatArg::Image => Format::Image,
            FormatArg::Zip => Format::Zip,
            FormatArg::Epub => Format::Epub,
            FormatArg::Audio => Format::Audio,
            FormatArg::Csv => Format::Csv,
            FormatArg::Html => Format::Html,
            FormatArg::Json => Format::Json,
            FormatArg::Yaml => Format::Yaml,
            FormatArg::Toml => Format::Toml,
            FormatArg::Xml => Format::Xml,
            FormatArg::Sqlite => Format::Sqlite,
            FormatArg::Tar => Format::Tar,
            FormatArg::Video => Format::Video,
            FormatArg::Ocr => Format::Ocr,
            FormatArg::MarkdownDocx => Format::MarkdownDocx,
            FormatArg::Rst => Format::Rst,
            FormatArg::Org => Format::Org,
            FormatArg::Mediawiki => Format::MediaWiki,
            FormatArg::Asciidoc => Format::Asciidoc,
        }
    }
}

fn resolve_output_format(detected: Format, forced_to: Option<&ToArg>) -> miette::Result<Format> {
    match forced_to {
        None => Ok(detected),
        Some(to) => {
            if detected == Format::MarkdownDocx {
                Ok(to.clone().into())
            } else {
                Err(miette::miette!(
                    "--to is only valid for Markdown (.md) input files"
                ))
            }
        }
    }
}

#[cfg_attr(not(feature = "ocr"), allow(unused_variables))]
fn resolve_converter(
    format: Format,
    ocr_lang: &str,
) -> mq_conv::error::Result<Box<dyn mq_conv::converter::Converter>> {
    match format {
        #[cfg(feature = "ocr")]
        Format::Ocr => Ok(mq_conv::formats::get_ocr_converter(ocr_lang)),
        _ => mq_conv::formats::get_converter(format),
    }
}

/// Converted Markdown (or other target format) plus the output file extension.
struct Converted {
    bytes: Vec<u8>,
    extension: &'static str,
    /// False for binary targets (DOCX, EPUB), which must be written verbatim.
    is_text: bool,
}

fn convert_bytes(
    input: &[u8],
    filename: Option<&str>,
    args: &Args,
    image_dir: Option<PathBuf>,
    image_link_dir: Option<PathBuf>,
) -> miette::Result<Converted> {
    let detected = if let Some(f) = args.format.as_ref() {
        f.clone().into()
    } else {
        Format::detect(filename, input).ok_or_else(|| {
            miette::miette!("Could not detect file format. Use --format to specify.")
        })?
    };
    let format = resolve_output_format(detected, args.to.as_ref())?;
    let converter =
        resolve_converter(format, &args.ocr_lang).map_err(|e| miette::miette!("{e}"))?;

    let options = ConvertOptions {
        image_dir,
        image_link_dir,
        ocr_lang: Some(args.ocr_lang.clone()),
    };
    let mut bytes = Vec::new();
    converter
        .convert_with(input, &mut bytes, &options)
        .map_err(|e| miette::miette!("{e}"))?;
    Ok(Converted {
        bytes,
        extension: converter.output_extension(),
        is_text: converter.is_text_output(),
    })
}

/// Apply `--cursor` / `--max-tokens` to a finished conversion.
fn write_budgeted(
    converted: &Converted,
    args: &Args,
    writer: &mut dyn Write,
) -> miette::Result<()> {
    if args.max_tokens.is_none() && args.cursor == 0 {
        writer.write_all(&converted.bytes).into_diagnostic()?;
        return Ok(());
    }
    if !converted.is_text {
        return Err(miette::miette!(
            "--max-tokens and --cursor only apply to text output; .{} output is binary",
            converted.extension
        ));
    }
    let text = String::from_utf8_lossy(&converted.bytes);
    let chunk = budget::take_chunk(&text, args.cursor, args.max_tokens)
        .map_err(|e| miette::miette!("{e}"))?;
    writer.write_all(chunk.text.as_bytes()).into_diagnostic()?;
    if let Some(next) = chunk.next_cursor {
        if !chunk.text.ends_with('\n') {
            writeln!(writer).into_diagnostic()?;
        }
        writeln!(
            writer,
            "\n<!-- mq-conv: next-cursor={next} chunk-tokens={} total-tokens={} -->",
            chunk.tokens, chunk.total_tokens
        )
        .into_diagnostic()?;
    }
    Ok(())
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string())
}

/// Where extracted images for `path` go: the given directory, or a
/// per-file sub-directory when several inputs share it.
fn image_dir_for(args: &Args, path: &Path) -> Option<PathBuf> {
    let base = args.extract_images.as_ref()?;
    Some(if args.files.len() > 1 {
        base.join(file_stem(path))
    } else {
        base.clone()
    })
}

/// Directory prefix for image links in Markdown that is saved to
/// `--output-dir`: links must resolve from the output file, not the cwd.
fn image_link_dir_for(args: &Args, image_dir: &Path) -> Option<PathBuf> {
    let output_dir = args.output_dir.as_ref()?;
    Some(relative_path(output_dir, image_dir))
}

fn convert_file(path: &Path, args: &Args) -> miette::Result<Converted> {
    let input = fs::read(path).into_diagnostic()?;
    let filename = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let image_dir = image_dir_for(args, path);
    let image_link_dir = image_dir
        .as_deref()
        .and_then(|dir| image_link_dir_for(args, dir));
    convert_bytes(&input, filename.as_deref(), args, image_dir, image_link_dir)
        .map_err(|e| miette::miette!("{}: {e}", path.display()))
}

fn main() -> miette::Result<()> {
    let args = Args::parse();

    if (args.max_tokens.is_some() || args.cursor > 0) && args.files.len() > 1 {
        return Err(miette::miette!(
            "--max-tokens and --cursor work on a single input; got {} files",
            args.files.len()
        ));
    }
    if args.max_tokens.is_some() && args.output_dir.is_some() {
        return Err(miette::miette!(
            "--max-tokens cannot be combined with --output-dir"
        ));
    }

    if args.files.is_empty() {
        // stdin mode
        if io::stdin().is_terminal() {
            return Err(miette::miette!(
                "No input file specified and stdin is a terminal.\nUsage: mq-conv <FILE>... or pipe data to stdin with --format"
            ));
        }
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf).into_diagnostic()?;

        let converted = convert_bytes(&buf, None, &args, args.extract_images.clone(), None)?;
        let stdout = io::stdout();
        let mut writer = BufWriter::new(stdout.lock());
        write_budgeted(&converted, &args, &mut writer)?;
        writer.flush().into_diagnostic()?;
        return Ok(());
    }

    // Files are converted in parallel; results are emitted in input order.
    let results: Vec<Option<miette::Result<Converted>>> =
        par_map(&args.files, |path| convert_file(path, &args));

    if let Some(ref output_dir) = args.output_dir {
        fs::create_dir_all(output_dir).into_diagnostic()?;
        for (path, result) in args.files.iter().zip(results) {
            let converted = result
                .ok_or_else(|| miette::miette!("{}: conversion panicked", path.display()))??;
            let out_path = output_dir.join(format!("{}.{}", file_stem(path), converted.extension));
            fs::write(&out_path, &converted.bytes).into_diagnostic()?;
        }
    } else {
        let stdout = io::stdout();
        let mut writer = BufWriter::new(stdout.lock());
        for (i, (path, result)) in args.files.iter().zip(results).enumerate() {
            if i > 0 {
                writeln!(writer, "\n---\n").into_diagnostic()?;
            }
            let converted = result
                .ok_or_else(|| miette::miette!("{}: conversion panicked", path.display()))??;
            write_budgeted(&converted, &args, &mut writer)?;
        }
        writer.flush().into_diagnostic()?;
    }

    Ok(())
}

use std::fs;
use std::io::{self, BufWriter, IsTerminal, Read, Write};
use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use miette::IntoDiagnostic;

use mq_conv::detect::Format;

#[derive(Parser, Debug)]
#[command(name = "mq-conv")]
#[command(version, about = "Convert various file formats to Markdown")]
struct Args {
    /// Input file paths (reads from stdin if not provided)
    files: Vec<PathBuf>,

    /// Force a specific format instead of auto-detecting
    #[arg(short, long)]
    format: Option<FormatArg>,

    /// Output directory for individual .md files (one per input file)
    #[arg(short, long)]
    output_dir: Option<PathBuf>,
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
        }
    }
}

fn convert_one(
    input: &[u8],
    filename: Option<&str>,
    forced_format: Option<&FormatArg>,
    writer: &mut dyn Write,
) -> miette::Result<()> {
    let format = if let Some(f) = forced_format {
        f.clone().into()
    } else {
        Format::detect(filename, input).ok_or_else(|| {
            miette::miette!("Could not detect file format. Use --format to specify.")
        })?
    };

    let converter = mq_conv::formats::get_converter(format).map_err(|e| miette::miette!("{e}"))?;
    converter
        .convert(input, writer)
        .map_err(|e| miette::miette!("{e}"))?;
    Ok(())
}

fn main() -> miette::Result<()> {
    let args = Args::parse();

    if args.files.is_empty() {
        // stdin mode
        if io::stdin().is_terminal() {
            return Err(miette::miette!(
                "No input file specified and stdin is a terminal.\nUsage: mq-conv <FILE>... or pipe data to stdin with --format"
            ));
        }
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf).into_diagnostic()?;

        let stdout = io::stdout();
        let mut writer = BufWriter::new(stdout.lock());
        convert_one(&buf, None, args.format.as_ref(), &mut writer)?;
        writer.flush().into_diagnostic()?;
    } else if let Some(ref output_dir) = args.output_dir {
        // Output each file as individual .md file
        fs::create_dir_all(output_dir).into_diagnostic()?;

        for path in &args.files {
            let input = fs::read(path).into_diagnostic()?;
            let filename = path.file_name().map(|n| n.to_string_lossy().into_owned());

            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "output".to_string());
            let out_path = output_dir.join(format!("{stem}.md"));

            let file = fs::File::create(&out_path).into_diagnostic()?;
            let mut writer = BufWriter::new(file);
            convert_one(
                &input,
                filename.as_deref(),
                args.format.as_ref(),
                &mut writer,
            )?;
            writer.flush().into_diagnostic()?;
        }
    } else {
        // Output all to stdout
        let stdout = io::stdout();
        let mut writer = BufWriter::new(stdout.lock());

        for (i, path) in args.files.iter().enumerate() {
            if i > 0 {
                writeln!(writer, "\n---\n").into_diagnostic()?;
            }
            let input = fs::read(path).into_diagnostic()?;
            let filename = path.file_name().map(|n| n.to_string_lossy().into_owned());
            convert_one(
                &input,
                filename.as_deref(),
                args.format.as_ref(),
                &mut writer,
            )?;
        }
        writer.flush().into_diagnostic()?;
    }

    Ok(())
}

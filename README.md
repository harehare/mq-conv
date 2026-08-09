<h1 align="center">mq-conv</h1>

<p align="center"><b>Turn PDFs, Office docs, and 20+ other formats into clean, structure-preserving Markdown.</b></p>

<div align="center">

[![ci](https://img.shields.io/github/actions/workflow/status/harehare/mq-conv/ci.yml?style=flat-square&logo=github-actions&label=ci)](https://github.com/harehare/mq-conv/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/mq-conv?logo=rust&style=flat-square)](https://crates.io/crates/mq-conv)
[![LICENCE](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](LICENSE)

</div>

`mq-conv` is a command-line tool, written in Rust, that converts PDFs, Office documents, markup languages, spreadsheets, data formats, media, and archives into Markdown — locally, with no API keys, no network calls, and no Python runtime. It ships as a single static binary and is designed to be the first stage of a Unix-style pipeline with [mq](https://github.com/harehare/mq), the jq-for-Markdown query engine.

## Why mq-conv?

Most "convert X to Markdown" tools either dump raw text (losing headings, tables, and links) or require a cloud API call to an LLM-powered parser. mq-conv takes a middle path: **layout-aware, offline parsing** that reconstructs document structure directly from the file format, so the Markdown it produces is something you can actually query, diff, and feed to an LLM.

- **Local & offline** — no API keys, no network access, no rate limits. Your files never leave your machine.
- **Single static binary** — install once, use anywhere. No Python/Node runtime or dependency hell.
- **Structure-aware, not text-dump** — headings, tables, lists, and hyperlinks are reconstructed from layout and document XML, not just concatenated text.
- **20+ formats** — documents, markup languages, spreadsheets, data formats, media, and archives, all through one CLI.
- **Composable** — output is plain Markdown on stdout, ready to pipe into [mq](https://github.com/harehare/mq) or any other Unix tool.

### Key Features

- **Layout-Aware PDF Parsing** - Reconstructs headings (by relative font size), paragraphs, lists, and tables from glyph positions, and strips repeated running headers/footers and page numbers
- **Hyperlink Preservation** - Word documents keep their links as `[text](url)` Markdown instead of dropping them
- **Automatic Format Detection** - Detects file formats by extension and magic bytes
- **20+ Supported Formats** - Documents, markup, data, media, and archives
- **Image OCR** - Extract text from images using Tesseract OCR
- **Markdown to Word** - Convert Markdown documents to `.docx` format
- **Stdin Support** - Pipe data directly from other commands
- **Modular Architecture** - Enable only the formats you need via feature flags

## Installation

### Using the Installation Script (Recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/harehare/mq-conv/main/bin/install.sh | bash
```

The installer will:
- Download the latest release for your platform
- Verify the binary with SHA256 checksum
- Install to `~/.mq-conv/bin/`
- Update your shell profile (bash, zsh, or fish)

After installation, restart your terminal or run:

```bash
source ~/.bashrc  # or ~/.zshrc, or ~/.config/fish/config.fish
```

### Cargo

```bash
# Install from crates.io
cargo install mq-conv
# Install using binstall (grabs the latest prebuilt release)
cargo binstall mq-conv
```

### From Source

```bash
git clone https://github.com/harehare/mq-conv.git
cd mq-conv
cargo build --release
# Binary will be at target/release/mq-conv
```

Pre-built binaries for macOS, Linux, and Windows are also available on the [GitHub releases page](https://github.com/harehare/mq-conv/releases).

## Usage

### Basic Usage

```bash
# Convert a file to Markdown
mq-conv input.pdf

# Force a specific format
mq-conv input.bin --format json

# Pipe from stdin
cat input.json | mq-conv --format json

# Convert a whole batch of files into a directory of .md files
mq-conv reports/*.pdf notes/*.docx --output-dir ./out
```

### Combine with mq

`mq-conv` is designed to be the first stage of an `mq` pipeline: convert to structured Markdown, then slice, filter, and transform it with [mq](https://github.com/harehare/mq)'s jq-like query language.

```bash
# Convert a PDF and query headings
mq conv document.pdf | mq '.h'

# Convert Excel and filter content
mq conv data.xlsx | mq '.table'

# Convert a Word doc and pull out one section
mq conv document.docx | mq -A 'section::section("Summary")'

# Convert slides and view them directly in the terminal
mq conv slides.pptx | mq view
```

## Supported Formats

### Documents

| Format          | Extensions          | Notes                                              |
| ---------------- | -------------------- | --------------------------------------------------- |
| PDF               | `.pdf`               | Layout-aware: headings, paragraphs, lists, tables    |
| Word              | `.docx`              | Headings, styles, lists, tables                      |
| PowerPoint        | `.pptx`              | Slides, titles, bullets, tables, speaker notes        |
| EPUB              | `.epub`               | Chapter structure preserved                          |
| HTML              | `.html`               | Via [mq-markdown](https://github.com/harehare/mq)     |
| Markdown → Word   | `.md`, `.markdown`    | Reverse conversion, see `--to`                        |

### Markup Languages

| Format            | Extensions            |
| ------------------ | ---------------------- |
| reStructuredText   | `.rst`                 |
| Org-mode           | `.org`                 |
| MediaWiki          | `.wiki`, `.mediawiki`  |
| AsciiDoc           | `.adoc`, `.asciidoc`   |

### Spreadsheets

| Format | Extensions                       |
| ------ | --------------------------------- |
| Excel  | `.xlsx`, `.xls`, `.xlsb`, `.ods`  |
| CSV    | `.csv`, `.tsv`                    |

### Data Formats

| Format | Extensions                    |
| ------ | ------------------------------ |
| JSON   | `.json`                        |
| YAML   | `.yaml`, `.yml`                |
| TOML   | `.toml`                        |
| XML    | `.xml`                         |
| SQLite | `.sqlite`, `.sqlite3`, `.db`   |

### Media

| Format | Extensions                                                          |
| ------ | --------------------------------------------------------------------- |
| Image  | `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.svg`, `.bmp`, `.tiff`      |
| OCR    | any image (use `--format ocr`)                                        |
| Audio  | `.mp3`, `.wav`, `.flac`, `.ogg`, `.m4a`, `.aac`, `.wma`                |
| Video  | `.mp4`, `.mkv`, `.avi`, `.mov`, `.webm`, `.m4v`, `.wmv`, `.flv`        |

### Archives

| Format | Extensions      |
| ------ | ---------------- |
| ZIP    | `.zip`            |
| TAR    | `.tar`, `.tgz`    |

## How Conversion Works

mq-conv doesn't just extract raw text — each converter reconstructs Markdown structure from the source format's own layout signals:

- **PDF**: glyphs are collected with their position and font size, grouped into words and lines, then reassembled into paragraphs, bulleted/numbered lists, and tables by clustering x-positions into column boundaries. Headings are detected from font size relative to the document's body text, hyphenated line breaks are rejoined, and lines that repeat in the same margin position across most pages (running headers, footers, page numbers) are stripped out.
- **Word / PowerPoint**: parsed directly from the underlying OOXML (`document.xml` / `slideN.xml`), preserving heading styles, bold/italic runs, hyperlinks, list nesting, and table structure — no shelling out to Office or LibreOffice.
- **Excel**: each sheet is segmented into blocks by blank rows, and each block is classified as a table or free text before rendering, so title cells and footnotes don't get mangled into table columns.
- **HTML**: delegated to [mq-markdown](https://github.com/harehare/mq)'s HTML-to-Markdown engine, which handles semantic tags, code blocks, and front matter.

This is the same philosophy as document-parsing tools like Pandoc or Firecrawl's document ingestion, but implemented as dependency-free Rust so it runs anywhere, instantly, with no external service.

## Command-Line Options

```
Usage: mq-conv [OPTIONS] [FILES]...

Arguments:
  [FILES]...  Input file paths (reads from stdin if omitted)

Options:
  -f, --format <FORMAT>          Force a specific format instead of auto-detecting
  -o, --output-dir <OUTPUT_DIR>  Output directory for individual output files (one per input file)
      --to <TO>                  Target output format when converting from Markdown
  -h, --help                     Print help
  -V, --version                  Print version
```

### Available Format Values

`excel`, `pdf`, `powerpoint`, `word`, `image`, `zip`, `epub`, `audio`, `csv`, `html`, `json`, `yaml`, `toml`, `xml`, `sqlite`, `tar`, `video`, `ocr`, `markdown-docx`, `rst`, `org`, `mediawiki`, `asciidoc`

### OCR Requirements

The `ocr` feature requires Tesseract to be installed on your system:

```bash
# macOS
brew install tesseract

# Ubuntu/Debian
sudo apt install tesseract-ocr

# Arch Linux
sudo pacman -S tesseract
```

Usage:

```bash
# OCR an image to Markdown
mq-conv photo.png --format ocr

# Convert Markdown to Word docx
mq-conv document.md
mq-conv document.md --output-dir ./out  # creates document.docx
```

## Related Projects

- [mq](https://github.com/harehare/mq) - The underlying Markdown query processor
- [mq-tui](https://github.com/harehare/mq-tui) - Interactive terminal interface for mq
- [mqlang.org](https://mqlang.org) - Documentation and language reference

## Support

- 🐛 [Report bugs](https://github.com/harehare/mq-conv/issues/new)
- 💡 [Request features](https://github.com/harehare/mq-conv/issues/new)
- ⭐ [Star the project](https://github.com/harehare/mq-conv) if you find it useful!

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

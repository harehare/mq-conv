<div align="center">
  <img src="assets/logo.svg" width="80" height="80" alt="mq-conv logo" /><br/>
</div>

<h1 align="center">mq-conv</h1>

<p align="center"><b>Turn PDFs, Office docs, and 20+ other formats into clean, structure-preserving Markdown.</b></p>

<div align="center">

[![ci](https://img.shields.io/github/actions/workflow/status/harehare/mq-conv/ci.yml?style=flat-square&logo=github-actions&label=ci)](https://github.com/harehare/mq-conv/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/mq-conv?logo=rust&style=flat-square)](https://crates.io/crates/mq-conv)
[![LICENCE](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](LICENSE)

</div>

`mq-conv` converts PDFs, Office docs, markup, spreadsheets, data formats, media, and archives into Markdown — locally, no API keys, no network calls, no Python runtime. Single static binary, built to feed [mq](https://github.com/harehare/mq), the jq-for-Markdown query engine.

## Why mq-conv?

Most "convert to Markdown" tools either dump raw text or call out to a cloud LLM parser. mq-conv takes a middle path: **layout-aware, offline parsing** that reconstructs structure from the file format itself, so the output is Markdown you can actually query, diff, and feed to an LLM.

- **Local & offline** — no API keys, no network, no rate limits
- **Single static binary** — no Python/Node runtime, no dependency hell
- **Structure-aware** — headings, tables, lists, links reconstructed from layout, not a text dump
- **20+ formats** — documents, markup, spreadsheets, data, media, archives, one CLI
- **Composable** — plain Markdown on stdout, ready to pipe into [mq](https://github.com/harehare/mq)

### Key Features

- **Layout-Aware PDF Parsing** — headings, paragraphs, lists, tables from glyph positions; strips repeated headers/footers/page numbers
- **Hyperlink Preservation** — Word links survive as `[text](url)`
- **Automatic Format Detection** — by extension and magic bytes
- **20+ Supported Formats** — documents, markup, data, media, archives
- **Image OCR** — via Tesseract
- **Markdown to Word** — convert `.md` to `.docx`
- **Stdin Support** — pipe data from other commands
- **Modular Architecture** — enable only the formats you need via feature flags
- **WebAssembly** — core formats also build for the browser, see [WebAssembly](#webassembly)

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

| Format | Extensions | Notes |
| --- | --- | --- |
| PDF | `.pdf` | Layout-aware: headings, paragraphs, lists, tables |
| Word | `.docx` | Headings, styles, lists, tables |
| PowerPoint | `.pptx` | Slides, titles, bullets, tables, speaker notes |
| EPUB | `.epub` | Chapter structure preserved |
| HTML | `.html` | Via [mq-markdown](https://github.com/harehare/mq) |
| Markdown → Word | `.md`, `.markdown` | Reverse conversion, see `--to` |

### Markup Languages

| Format | Extensions |
| --- | --- |
| reStructuredText | `.rst` |
| Org-mode | `.org` |
| MediaWiki | `.wiki`, `.mediawiki` |
| AsciiDoc | `.adoc`, `.asciidoc` |

### Spreadsheets

| Format | Extensions |
| --- | --- |
| Excel | `.xlsx`, `.xls`, `.xlsb`, `.ods` |
| CSV | `.csv`, `.tsv` |

### Data Formats

| Format | Extensions |
| --- | --- |
| JSON | `.json` |
| YAML | `.yaml`, `.yml` |
| TOML | `.toml` |
| XML | `.xml` |
| SQLite | `.sqlite`, `.sqlite3`, `.db` |

### Media

| Format | Extensions |
| --- | --- |
| Image | `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.svg`, `.bmp`, `.tiff` |
| OCR | any image (use `--format ocr`) |
| Audio | `.mp3`, `.wav`, `.flac`, `.ogg`, `.m4a`, `.aac`, `.wma` |
| Video | `.mp4`, `.mkv`, `.avi`, `.mov`, `.webm`, `.m4v`, `.wmv`, `.flv` |

### Archives

| Format | Extensions |
| --- | --- |
| ZIP | `.zip` |
| TAR | `.tar`, `.tgz` |

## How Conversion Works

Each converter reconstructs Markdown structure from the source format's own layout signals, not just raw text:

- **PDF** — glyphs are grouped by position/font into words, lines, paragraphs, lists, and tables (via x-position clustering). Headings come from relative font size; repeated headers/footers/page numbers are stripped.
- **Word / PowerPoint** — parsed directly from OOXML (`document.xml` / `slideN.xml`): headings, bold/italic, hyperlinks, list nesting, tables. No Office/LibreOffice shell-out.
- **Excel** — each sheet is split into blocks by blank rows, then each block is classified as table or free text.
- **HTML** — delegated to [mq-markdown](https://github.com/harehare/mq)'s HTML-to-Markdown engine.

Same philosophy as Pandoc or Firecrawl's ingestion, but dependency-free Rust — runs anywhere, instantly, no external service.

## Command-Line Options

```
Usage: mq-conv [OPTIONS] [FILES]...

Arguments:
  [FILES]...  Input file paths (reads from stdin if omitted)

Options:
  -f, --format <FORMAT>          Force a specific format instead of auto-detecting
  -o, --output-dir <OUTPUT_DIR>  Output directory for individual output files (one per input file)
      --to <TO>                  Target output format when converting from Markdown
      --ocr-lang <OCR_LANG>      Tesseract language for OCR, e.g. "jpn" or "eng+jpn" [default: eng]
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

OCR defaults to English. For other languages, install the matching Tesseract language pack and pass `--ocr-lang`:

```bash
# macOS: installs all language packs
brew install tesseract-lang

# Ubuntu/Debian: install just what you need
sudo apt install tesseract-ocr-jpn tesseract-ocr-chi-sim tesseract-ocr-kor

# OCR a Japanese image (language codes can be combined with "+")
mq-conv photo.png --format ocr --ocr-lang jpn
mq-conv mixed.png --format ocr --ocr-lang eng+jpn
```

Usage:

```bash
# OCR an image to Markdown
mq-conv photo.png --format ocr

# Convert Markdown to Word docx
mq-conv document.md
mq-conv document.md --output-dir ./out  # creates document.docx
```

## WebAssembly

The `wasm` feature builds `mq-conv` as a `cdylib` for `wasm32-unknown-unknown`, for use in the browser via [wasm-bindgen](https://github.com/rustwasm/wasm-bindgen). It bundles every format except `sqlite` and `ocr`, which depend on native C libraries.

```bash
wasm-pack build --target web --no-default-features --features wasm --release
```

| Feature | Formats | Size (gzip) |
| --- | --- | --- |
| `wasm` | everything but `sqlite`/`ocr` | ~2.7 MB |
| `wasm_slim` | drops `image`, `audio`, `video`, `tar` — documents/markup/data only | ~2.3 MB |

Use `wasm_slim` for document-conversion-only use cases:

```bash
wasm-pack build --target web --no-default-features --features wasm_slim --release
```

```js
import init, { convert, detectFormat } from "./pkg/mq_conv.js";

await init();
const bytes = new Uint8Array(await file.arrayBuffer());
const markdown = convert(bytes, file.name, undefined); // filename, or pass a format name directly
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

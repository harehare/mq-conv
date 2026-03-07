<h1 align="center">mq-conv</h1>

[![ci](https://github.com/harehare/mq-conv/actions/workflows/ci.yml/badge.svg)](https://github.com/harehare/mq-conv/actions/workflows/ci.yml)
[![mq language](https://img.shields.io/badge/mq-language-orange.svg)](https://github.com/harehare/mq)

A CLI tool for converting various file formats to Markdown

## Overview

`mq-conv` is a command-line tool that converts various file formats to Markdown. It supports 16+ formats including documents, spreadsheets, data formats, media files, and archives. Designed to work seamlessly with [mq](https://github.com/harehare/mq) and other Markdown processing tools.

### Key Features

- **Automatic Format Detection** - Detects file formats by extension and magic bytes
- **18+ Supported Formats** - Documents, data, media, and archives
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
# Install using binstall
cargo binstall mq-conv@0.1.0
```

### From Source

```bash
git clone https://github.com/harehare/mq-conv.git
cd mq-conv
cargo build --release
# Binary will be at target/release/mq-conv
```

## Usage

### Basic Usage

```bash
# Convert a file to Markdown
mq-conv input.pdf

# Force a specific format
mq-conv input.bin --format json

# Pipe from stdin
cat input.json | mq-conv --format json
```

### Combine with mq

```bash
# Convert a PDF and query headings
mq conv document.pdf | mq '.h'

# Convert Excel and filter content
mq conv data.xlsx | mq '.table'
```

## Supported Formats

### Documents

| Format          | Extensions         |
| --------------- | ------------------ |
| Word            | `.docx`            |
| PowerPoint      | `.pptx`            |
| PDF             | `.pdf`             |
| EPUB            | `.epub`            |
| HTML            | `.html`            |
| Markdown → Word | `.md`, `.markdown` |

### Spreadsheets

| Format | Extensions                       |
| ------ | -------------------------------- |
| Excel  | `.xlsx`, `.xls`, `.xlsb`, `.ods` |
| CSV    | `.csv`, `.tsv`                   |

### Data Formats

| Format | Extensions                   |
| ------ | ---------------------------- |
| JSON   | `.json`                      |
| YAML   | `.yaml`, `.yml`              |
| TOML   | `.toml`                      |
| XML    | `.xml`                       |
| SQLite | `.sqlite`, `.sqlite3`, `.db` |

### Media

| Format | Extensions                                                        |
| ------ | ----------------------------------------------------------------- |
| Image  | `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.svg`, `.bmp`, `.tiff` |
| OCR    | any image (use `--format ocr`)                                    |
| Audio  | `.mp3`, `.wav`, `.flac`, `.ogg`, `.m4a`, `.aac`, `.wma`           |
| Video  | `.mp4`, `.mkv`, `.avi`, `.mov`, `.webm`, `.m4v`, `.wmv`, `.flv`   |

### Archives

| Format | Extensions     |
| ------ | -------------- |
| ZIP    | `.zip`         |
| TAR    | `.tar`, `.tgz` |

## Command-Line Options

```
Usage: mq-conv [OPTIONS] [FILE]

Arguments:
  [FILE]  Input file path (reads from stdin if omitted)

Options:
  -f, --format <FORMAT>  Force a specific format instead of auto-detecting
  -h, --help             Print help
  -V, --version          Print version
```

### Available Format Values

`excel`, `pdf`, `powerpoint`, `word`, `image`, `zip`, `epub`, `audio`, `csv`, `html`, `json`, `yaml`, `toml`, `xml`, `sqlite`, `tar`, `video`, `ocr`, `markdown-docx`

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

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.

## License

MIT

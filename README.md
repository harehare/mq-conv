<h1 align="center">mq-conv</h1>

[![ci](https://github.com/harehare/mq-conv/actions/workflows/ci.yml/badge.svg)](https://github.com/harehare/mq-conv/actions/workflows/ci.yml)
[![mq language](https://img.shields.io/badge/mq-language-orange.svg)](https://github.com/harehare/mq)

<div align="center">

A CLI tool for converting various file formats to Markdown

</div>

## Overview

`mq-conv` is a command-line tool that converts various file formats to Markdown. It supports 16+ formats including documents, spreadsheets, data formats, media files, and archives. Designed to work seamlessly with [mq](https://github.com/harehare/mq) and other Markdown processing tools.

### Key Features

- **Automatic Format Detection** - Detects file formats by extension and magic bytes
- **16+ Supported Formats** - Documents, data, media, and archives
- **Stdin Support** - Pipe data directly from other commands
- **Modular Architecture** - Enable only the formats you need via feature flags

## Installation

```bash
# Install from crates.io
cargo install mq-conv

# Install from source
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
mq-conv document.pdf | mq '.h'

# Convert Excel and filter content
mq-conv data.xlsx | mq '.table'
```

## Supported Formats

### Documents

| Format     | Extensions |
| ---------- | ---------- |
| Word       | `.docx`    |
| PowerPoint | `.pptx`    |
| PDF        | `.pdf`     |
| EPUB       | `.epub`    |
| HTML       | `.html`    |

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

`excel`, `pdf`, `powerpoint`, `word`, `image`, `zip`, `epub`, `audio`, `csv`, `html`, `json`, `yaml`, `toml`, `xml`, `sqlite`, `tar`, `video`

## Related Projects

- [mq](https://github.com/harehare/mq) - The underlying Markdown query processor
- [mq-tui](https://github.com/harehare/mq-tui) - Interactive terminal interface for mq
- [mqlang.org](https://mqlang.org) - Documentation and language reference

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.

## License

MIT

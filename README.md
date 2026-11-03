<div align="center">
  <img src="assets/logo.svg" width="96" height="96" alt="mq-conv logo" />

<a href="https://conv.mqlang.org">Visit the site 🌐</a>
—
<a href="https://conv.mqlang.org/book/">Read the book 📖</a>

  <h1>mq-conv</h1>

[![ci](https://img.shields.io/github/actions/workflow/status/harehare/mq-conv/ci.yml?style=flat-square&logo=github-actions&label=ci)](https://github.com/harehare/mq-conv/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/mq-conv?logo=rust&style=flat-square)](https://crates.io/crates/mq-conv)
[![license](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](LICENSE)

</div>

`mq-conv` turns PDFs, Office docs and other formats into clean, structure-preserving Markdown. It runs locally: no API keys, no network, no Python runtime. It is built to feed [mq](https://github.com/harehare/mq), the jq-like query tool for Markdown.

## Features

- [Layout-aware PDF parsing](https://conv.mqlang.org/book/guide/how-it-works.html): columns, headings, lists, ruled and borderless tables, CJK fonts
- [Many formats](https://conv.mqlang.org/book/guide/formats.html): documents, markup, spreadsheets, data, media, archives
- [Built-in mq queries](https://conv.mqlang.org/book/guide/queries.html), [token budgeting](https://conv.mqlang.org/book/guide/tokens.html) and [page selection](https://conv.mqlang.org/book/guide/pages.html)
- [Image extraction and OCR](https://conv.mqlang.org/book/guide/images-ocr.html), [Any input to docx, HTML, EPUB and more](https://conv.mqlang.org/book/guide/markdown-out.html)
- Parallel conversion, per-format [feature flags](https://conv.mqlang.org/book/reference/features.html) and a [WebAssembly](https://conv.mqlang.org/book/reference/wasm.html) build

## Installation

```bash
curl -fsSL https://raw.githubusercontent.com/harehare/mq-conv/main/bin/install.sh | bash
```

or with Cargo:

```bash
cargo install mq-conv
cargo binstall mq-conv   # prebuilt release
```

## Usage

```bash
mq-conv report.pdf                                # convert to stdout
cat notes.org | mq-conv --format org              # stdin
mq-conv reports/*.pdf --output-dir ./out          # many files, one .md each
mq-conv report.pdf --pages 3-5                    # selected pages only
mq-conv report.pdf -q '.h2'                       # print what an mq query selects
mq-conv book.pdf --max-tokens 4000                # prompt-sized chunk, with a resume cursor
mq-conv report.pdf --extract-images ./img         # keep embedded images
mq conv document.docx | mq -A 'section::section("Summary")'   # pipe into mq
```

## Documentation

- [Quick Start](https://conv.mqlang.org/book/start/quickstart.html) and [Install](https://conv.mqlang.org/book/start/install.html)
- [Guide](https://conv.mqlang.org/book/guide/): formats, queries, tokens, pages, images and OCR
- [CLI](https://conv.mqlang.org/book/reference/cli.html), [feature flags](https://conv.mqlang.org/book/reference/features.html) and [WebAssembly](https://conv.mqlang.org/book/reference/wasm.html)

## License

MIT

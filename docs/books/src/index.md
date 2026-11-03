# mq-conv

`mq-conv` converts PDFs, Office docs, markup, spreadsheets, data formats, media and archives into Markdown. It runs locally: no API keys, no network calls, no Python runtime. It is a single binary, built to feed [mq](https://mqlang.org/), the jq-like query tool for Markdown.

Most "convert to Markdown" tools either dump raw text or call a cloud parser. mq-conv reconstructs structure (headings, tables, lists, links) from the file format's own layout, so the output is Markdown you can query, diff and give to an LLM.

```sh
mq-conv report.pdf                 # whole document
mq-conv report.pdf -q '.h2'        # only what a query selects
mq-conv book.pdf --max-tokens 4000 # one prompt-sized chunk at a time
```

## Features

- Layout-aware PDF parsing: columns, headings, lists, ruled and borderless tables, CJK fonts
- Documents, markup, spreadsheets, media and archives in one CLI, detected by extension and magic bytes
- Parallel conversion of PDF pages and input files
- [Token budgeting](guide/tokens.md) and [page selection](guide/pages.md)
- Built-in [mq queries](guide/queries.md)
- [Image extraction and OCR](guide/images-ocr.md)
- [Markdown to docx, HTML, LaTeX, EPUB and more](guide/markdown-out.md)
- Feature flags per format and a [WebAssembly](reference/wasm.md) build

Head to [Getting Started](start/index.md) to install it and convert your first file.

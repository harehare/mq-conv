# Feature Flags

Every format is a Cargo feature, all on by default. Build with `--no-default-features` and list only what you need to cut binary size:

```sh
cargo install mq-conv --no-default-features --features pdf,word
```

Features come in groups. The default build enables `documents`, `markup`, `writers`, `media`, `sqlite` and `query`.

| Feature | Enables |
| --- | --- |
| `documents` | `excel`, `pdf`, `powerpoint`, `word`, `epub`, `mobi`, `zip`, `ipynb`, `eml`, `opendocument` |
| `markup` | `rst`, `org`, `mediawiki`, `asciidoc` |
| `media` | `image`, `audio`, `video`, `tar` |
| `writers` | every `markdown_*` output target for `--to` |
| `sqlite` | SQLite databases (about 0.9 MB, needs a C library) |
| `query` | `-q` (about 2 MB) |
| `ocr` | Tesseract OCR (not default; needs Tesseract installed) |
| `wasm`, `wasm_slim` | [WebAssembly](wasm.md) bundles |

Approximate cost in the release binary: `query` 2.0 MB, `sqlite` 0.9 MB, `image` 0.5 MB, `writers` 0.5 MB, `audio` and `video` 0.2 MB together. The other formats add little.

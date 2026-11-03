# Feature Flags

Every format is a Cargo feature, all on by default. Build with `--no-default-features` and list only what you need to cut binary size:

```sh
cargo install mq-conv --no-default-features --features pdf,word,html
```

| Feature | Enables |
| --- | --- |
| `pdf`, `word`, `powerpoint`, `excel`, `epub` | the matching input format |
| `json`, `yaml`, `toml_conv`, `xml`, `sqlite` | data formats |
| `rst`, `org`, `mediawiki`, `asciidoc` | markup input |
| `image`, `audio`, `video`, `zip`, `tar` | media and archives |
| `markdown_*` | Markdown output targets for `--to` |
| `query` | `-q` (adds about 2.4 MB) |
| `ocr` | Tesseract OCR (not default; needs Tesseract installed) |
| `wasm`, `wasm_slim` | [WebAssembly](wasm.md) bundles |

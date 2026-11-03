# Supported Formats

The format is detected by extension and magic bytes. Override it with `--format`.

| Group | Format | Extensions | Notes |
| --- | --- | --- | --- |
| Documents | PDF | `.pdf` | Layout-aware: headings, paragraphs, lists, tables |
| | Word | `.docx` | Headings, styles, lists, tables, hyperlinks |
| | PowerPoint | `.pptx` | Slides, titles, bullets, tables, speaker notes |
| | EPUB | `.epub` | Chapter structure preserved |
| | HTML | `.html` | Via [mq-markdown](https://github.com/harehare/mq) |
| Markup | reStructuredText | `.rst` | |
| | Org-mode | `.org` | |
| | MediaWiki | `.wiki`, `.mediawiki` | |
| | AsciiDoc | `.adoc`, `.asciidoc` | |
| Spreadsheets | Excel | `.xlsx`, `.xls`, `.xlsb`, `.ods` | |
| | CSV | `.csv`, `.tsv` | |
| Data | JSON, YAML, TOML, XML | `.json`, `.yaml`, `.yml`, `.toml`, `.xml` | |
| | SQLite | `.sqlite`, `.sqlite3`, `.db` | |
| Media | Image | `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.svg`, `.bmp`, `.tiff` | OCR with `--format ocr` |
| | Audio | `.mp3`, `.wav`, `.flac`, `.ogg`, `.m4a`, `.aac`, `.wma` | |
| | Video | `.mp4`, `.mkv`, `.avi`, `.mov`, `.webm`, `.m4v`, `.wmv`, `.flv` | |
| Archives | ZIP, TAR | `.zip`, `.tar`, `.tgz` | |

`--format` values: `excel`, `pdf`, `powerpoint`, `word`, `image`, `zip`, `epub`, `audio`, `csv`, `html`, `json`, `yaml`, `toml`, `xml`, `sqlite`, `tar`, `video`, `ocr`, `markdown-docx`, `rst`, `org`, `mediawiki`, `asciidoc`.

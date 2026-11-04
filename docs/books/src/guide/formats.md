# Supported Formats

The format is detected by extension and magic bytes. Override it with `--format`.

| Group | Format | Extensions | Notes |
| --- | --- | --- | --- |
| Documents | PDF | `.pdf` | Layout-aware: headings, paragraphs, lists, tables |
| | Word | `.docx` | Headings, styles, lists, tables, hyperlinks |
| | PowerPoint | `.pptx` | Slides, titles, bullets, tables, speaker notes |
| | EPUB | `.epub` | Chapter structure preserved |
| | MOBI | `.mobi`, `.azw`, `.azw3`, `.prc` | Chapters, metadata, images. DRM-protected books are rejected |
| Markup | reStructuredText | `.rst` | |
| | Org-mode | `.org` | |
| | MediaWiki | `.wiki`, `.mediawiki` | |
| | AsciiDoc | `.adoc`, `.asciidoc` | |
| Spreadsheets | Excel | `.xlsx`, `.xls`, `.xlsb`, `.ods` | |
| Data | SQLite | `.sqlite`, `.sqlite3`, `.db` | |
| Media | Image | `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.svg`, `.bmp`, `.tiff` | OCR with `--format ocr` |
| | Audio | `.mp3`, `.wav`, `.flac`, `.ogg`, `.m4a`, `.aac`, `.wma` | |
| | Video | `.mp4`, `.mkv`, `.avi`, `.mov`, `.webm`, `.m4v`, `.wmv`, `.flv` | |
| Archives | ZIP, TAR | `.zip`, `.tar`, `.tgz` | |

`--format` values: `excel`, `pdf`, `powerpoint`, `word`, `image`, `zip`, `epub`, `mobi`, `audio`, `sqlite`, `tar`, `video`, `ocr`, `markdown-docx`, `rst`, `org`, `mediawiki`, `asciidoc`.

## CSV, HTML, JSON, YAML, TOML and XML

mq reads these directly, so mq-conv does not handle them:

```sh
mq -I csv 'include "csv" | csv_to_markdown_table(.)' data.csv
mq -I html '.' page.html
mq -I json '.' data.json   # also yaml, toml and xml
```

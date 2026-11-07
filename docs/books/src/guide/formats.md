# Supported Formats

The format is detected by extension and magic bytes. Override it with `--format`.

| Group | Format | Extensions | Notes |
| --- | --- | --- | --- |
| Documents | PDF | `.pdf` | Layout-aware: headings, paragraphs, lists, tables |
| | Word | `.docx` | Headings, styles, lists, tables, hyperlinks. Comments, footnotes and endnotes become [footnotes](#notes-comments-and-tracked-changes); tracked deletions are struck out |
| | PowerPoint | `.pptx` | Slides, titles, bullets, tables, speaker notes |
| | OpenDocument text | `.odt` | Headings, lists, tables, links, bold/italic, footnotes, comments, images |
| | OpenDocument presentation | `.odp` | Slides, titles, bullets, tables, speaker notes |
| | Jupyter Notebook | `.ipynb` | Markdown and code cells, text outputs, HTML tables, images (with `--extract-images`) |
| | E-mail | `.eml` | Headers, text body with quoted replies, attachment list |
| | EPUB | `.epub` | Chapter structure preserved |
| | MOBI | `.mobi`, `.azw`, `.azw3`, `.prc` | Chapters, metadata, images. DRM-protected books are rejected |
| Markup | reStructuredText | `.rst` | |
| | Org-mode | `.org` | |
| | MediaWiki | `.wiki`, `.mediawiki` | |
| | AsciiDoc | `.adoc`, `.asciidoc` | |
| Spreadsheets | Excel | `.xlsx`, `.xls`, `.xlsb`, `.ods` | Hidden sheets are marked; `--formulas` lists the formulas |
| Data | SQLite | `.sqlite`, `.sqlite3`, `.db` | |
| Media | Image | `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.svg`, `.bmp`, `.tiff` | OCR with `--format ocr` |
| | Audio | `.mp3`, `.wav`, `.flac`, `.ogg`, `.m4a`, `.aac`, `.wma` | |
| | Video | `.mp4`, `.mkv`, `.avi`, `.mov`, `.webm`, `.m4v`, `.wmv`, `.flv` | |
| Archives | ZIP, TAR | `.zip`, `.tar`, `.tgz` | |

`--format` values: `excel`, `pdf`, `powerpoint`, `word`, `odt`, `odp`, `ipynb`, `eml`, `image`, `zip`, `epub`, `mobi`, `audio`, `sqlite`, `tar`, `video`, `ocr`, `markdown-docx`, `rst`, `org`, `mediawiki`, `asciidoc`.

## Notes, comments and tracked changes

Word and OpenDocument notes come out as Markdown footnotes, defined after the text:

- Footnotes are `[^1]`, endnotes `[^e1]` and comments `[^c1]`. A comment starts with its author and date: `[^c1]: **Alice** (2024-05-06): Please check`.
- A tracked deletion in a `.docx` is kept as `~~struck out~~` text and an insertion as plain text, so the revision stays visible.

## PDF bookmarks

When a PDF has bookmarks (an outline), they set the heading levels: the line on the bookmark's page with the bookmark's title becomes a heading of the bookmark's depth. Documents without bookmarks keep the font-size heuristic.

## CSV, HTML, JSON, YAML, TOML and XML

mq reads these directly, so mq-conv does not handle them:

```sh
mq -I csv 'include "csv" | csv_to_markdown_table(.)' data.csv
mq -I html '.' page.html
mq -I json '.' data.json   # also yaml, toml and xml
```

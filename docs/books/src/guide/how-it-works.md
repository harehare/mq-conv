# How Conversion Works

Each converter reconstructs Markdown structure from the source format's own layout signals.

## PDF

`lopdf` only reads PDF objects and decompresses streams. The rest is done here from glyph coordinates:

- Fonts are decoded through `/ToUnicode`, `/Encoding` + `/Differences`, the embedded Type 1 / CFF font's own encoding, predefined CJK CMaps and Adobe-Japan1 CIDs, so Japanese PDFs without `/ToUnicode` still extract.
- Glyphs become words (by gaps measured against font size) and lines. Reading order comes from an XY-cut, so multi-column pages read column by column.
- Tables are detected from drawn rules (merged cells included) or, for borderless tables, from aligned whitespace columns.
- Headings come from relative font size, lists from bullet and number markers, code from monospaced fonts, links from annotations and contents pages from dot leaders.
- Repeated running headers, footers and page numbers are removed.

## Others

- **Word / PowerPoint**: parsed directly from OOXML (`document.xml`, `slideN.xml`): headings, bold/italic, hyperlinks, list nesting, tables. No Office shell-out.
- **Excel**: each sheet is split into blocks by blank rows, then each block is classified as table or free text.
- **HTML**: delegated to mq-markdown's HTML-to-Markdown engine.

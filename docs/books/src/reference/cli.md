# CLI

```text
Usage: mq-conv [OPTIONS] [FILES]...

Arguments:
  [FILES]...  Input file paths (reads from stdin if omitted)

Options:
  -f, --format <FORMAT>          Force a specific format instead of auto-detecting
  -o, --output-dir <OUTPUT_DIR>  Output directory for individual output files (one per input file)
      --to <TO>                  Write this format instead of Markdown (any input is converted through Markdown)
      --ocr-lang <OCR_LANG>      Tesseract language for OCR, e.g. "jpn" or "eng+jpn" [default: eng]
  -q, --query <QUERY>            Run an mq query over the converted Markdown and print its result
      --pages <PAGES>            Convert only these PDF pages, slides, sheets or EPUB/MOBI chapters, e.g. "3", "2-5", "7-" or "1,4-6"
      --formulas                 List spreadsheet formulas (Excel, ODS) after each sheet, besides the values
      --extract-images <DIR>     Extract embedded images (PDF, DOCX, PPTX, ODT, ODP, IPYNB, EPUB, MOBI) into DIR and link them
      --max-tokens <N>           Emit at most ~N tokens, cut at block boundaries (single input)
      --cursor <OFFSET>          Resume from the cursor printed by a previous --max-tokens run [default: 0]
  -h, --help                     Print help
  -V, --version                  Print version
```

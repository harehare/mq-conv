# Page Selection

`--pages` takes numbers and ranges (`3`, `2-5`, `7-`, `-4`, `1,4-6`) and works on PDF pages, PowerPoint slides, Excel sheets and EPUB and MOBI chapters. Other inputs reject it.

```sh
mq-conv report.pdf --pages 3-5
mq-conv report.pdf --pages 1,4-6,9-
mq-conv deck.pptx --pages 2-4
mq-conv budget.xlsx --pages 1
mq-conv novel.epub --pages 3
```

Slides and sheets are numbered by position. EPUB and MOBI chapters are numbered in reading order, counting only chapters that have text (the sections separated by `---` in the output). Numbers stay those of the source: PDF page markers, `# Slide N` headings and positions do not shift when others are skipped. A selection that matches nothing is an error.

For PDFs the whole document is still analysed, so heading levels and running-header removal match a full conversion. Only the selected pages are rendered, which saves time on OCR and image extraction. Unselected Excel sheets, PowerPoint slides and EPUB and MOBI chapters are never read or extracted.

# Combine with mq

`mq-conv` is the first stage of an `mq` pipeline: convert to structured Markdown, then slice, filter and transform it with [mq](https://mqlang.org/).

```sh
mq conv document.pdf | mq '.h'
mq conv data.xlsx | mq '.table'
mq conv document.docx | mq -A 'section::section("Summary")'
mq conv slides.pptx | mq view
```

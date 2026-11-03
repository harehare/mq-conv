# Markdown to Other Formats

A `.md` input is converted to Word by default. Pick another target with `--to`:

```sh
mq-conv document.md                       # document.docx
mq-conv document.md --output-dir ./out
mq-conv document.md --to html
```

Targets: `html`, `text`, `latex`, `rst`, `asciidoc`, `org`, `epub`, `json` (AST) and `docx`.

# Other Output Formats

Any input can be written in another format with `--to`. The input is converted to Markdown first, and that Markdown is written as the target:

```sh
mq-conv report.pdf --to html
mq-conv notes.org --to docx --output-dir ./out
mq-conv deck.pptx --to epub --output-dir ./out
```

A `.md` input is converted to Word by default:

```sh
mq-conv document.md                       # document.docx
mq-conv document.md --output-dir ./out
mq-conv document.md --to html
```

Targets: `html`, `text`, `latex`, `rst`, `asciidoc`, `org`, `epub`, `json` (AST) and `docx`. `--to` cannot be combined with `-q`.

# Quick Start

```sh
mq-conv input.pdf                  # convert a file to stdout
mq-conv input.bin --format rst     # force a format
cat notes.org | mq-conv --format org     # read stdin
```

Convert many files into a directory of `.md` files:

```sh
mq-conv reports/*.pdf notes/*.docx --output-dir ./out
```

Keep embedded images (written to `./img` and linked as `![image](img/...)`):

```sh
mq-conv report.pdf --extract-images ./img
```

Next: [query the result](../guide/queries.md), [page through large documents](../guide/tokens.md) or [pipe into mq](../guide/mq.md).

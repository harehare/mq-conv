# Queries

`-q/--query` runs an mq query over the converted Markdown and prints the result instead of the whole document. With several inputs the query runs on each file. With `--max-tokens` the query result is what gets paged. It needs Markdown output, so it cannot be combined with `--to`.

mq-conv adds three functions to the standard mq ones:

| Function | Returns |
| --- | --- |
| `tokens(x)` | estimated token count of a node or string, the same estimate `--max-tokens` uses |
| `format()` | the detected input format (`"pdf"`, `"word"`, ...) |
| `filename()` | the input file name, or `None` when reading stdin |

```sh
mq-conv report.pdf -q '.h2'
mq-conv report.pdf -q '.h2 | select(contains("Results"))'
mq-conv manual.pdf -q '.h2 | select(tokens(.) < 500)'
```

A query runs once per top-level node, as in `mq`. The `query` feature is on by default and adds about 2.4 MB to the release binary. Build with `--no-default-features` and pick formats to leave it out. It is not part of the WebAssembly builds.

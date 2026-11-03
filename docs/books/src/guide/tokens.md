# Token Budgeting

```sh
mq-conv book.pdf --max-tokens 4000
# ...ends with: <!-- mq-conv: next-cursor=18342 chunk-tokens=3921 total-tokens=187450 -->
mq-conv book.pdf --max-tokens 4000 --cursor 18342
```

`--max-tokens` cuts at block boundaries: paragraphs, tables and code blocks stay whole. It uses a built-in estimate of about 4 ASCII characters, or one CJK character, per token.

A cursor is a byte offset into the converted Markdown. Conversion is deterministic, so the same command with `--cursor` resumes exactly where the previous chunk stopped. Both options take a single input.

# WebAssembly

The `wasm` feature builds mq-conv as a `cdylib` for `wasm32-unknown-unknown`, for the browser via [wasm-bindgen](https://github.com/rustwasm/wasm-bindgen). It bundles every format except `sqlite` and `ocr`, which need native C libraries.

```sh
wasm-pack build --target web --no-default-features --features wasm --release
```

| Feature | Formats | Size (gzip) |
| --- | --- | --- |
| `wasm` | everything but `sqlite` and `ocr` | ~2.7 MB |
| `wasm_slim` | drops `image`, `audio`, `video`, `tar` | ~2.3 MB |

```js
import init, { convert, detectFormat } from "./pkg/mq_conv.js";

await init();
const bytes = new Uint8Array(await file.arrayBuffer());
const markdown = convert(bytes, file.name, undefined); // filename, or a format name directly
```

use wasm_bindgen::prelude::*;

use crate::detect::Format;

fn resolve_format(
    filename: Option<&str>,
    format: Option<&str>,
    bytes: &[u8],
) -> Result<Format, JsValue> {
    if let Some(name) = format {
        return Format::from_name(name)
            .ok_or_else(|| JsValue::from_str(&format!("Unknown format: {name}")));
    }
    Format::detect(filename, bytes).ok_or_else(|| {
        JsValue::from_str("Could not detect file format. Pass `format` to specify it explicitly.")
    })
}

#[wasm_bindgen]
pub fn convert(
    bytes: &[u8],
    filename: Option<String>,
    format: Option<String>,
) -> Result<String, JsValue> {
    let format = resolve_format(filename.as_deref(), format.as_deref(), bytes)?;
    let converter =
        crate::formats::get_converter(format).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut out = Vec::new();
    converter
        .convert(bytes, &mut out)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    String::from_utf8(out).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen(js_name = detectFormat)]
pub fn detect_format(bytes: &[u8], filename: Option<String>) -> Option<String> {
    Format::detect(filename.as_deref(), bytes).map(|f| f.to_string())
}

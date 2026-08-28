pub mod converter;
pub mod detect;
pub mod error;
pub mod formats;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

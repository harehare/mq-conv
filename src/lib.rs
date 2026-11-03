pub mod budget;
pub mod converter;
pub mod detect;
pub mod error;
pub mod formats;
pub mod parallel;
#[cfg(feature = "query")]
pub mod query;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

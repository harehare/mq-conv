use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("Format detection failed: could not determine file type")]
    DetectionFailed,

    #[error("Conversion error ({format}): {message}")]
    Conversion {
        format: &'static str,
        message: String,
    },

    #[error("Feature not enabled: {0}. Recompile with --features {0}")]
    FeatureDisabled(String),
}

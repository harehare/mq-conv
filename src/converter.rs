use crate::error::Result;
use std::io::Write;

pub trait Converter {
    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()>;
    fn format_name(&self) -> &'static str;
}

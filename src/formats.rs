#[cfg(any(feature = "json", feature = "toml_conv", feature = "yaml"))]
pub mod structured;

#[cfg(feature = "audio")]
pub mod audio;
#[cfg(feature = "csv")]
pub mod csv;
#[cfg(feature = "epub")]
pub mod epub;
#[cfg(feature = "excel")]
pub mod excel;
#[cfg(feature = "html")]
pub mod html;
#[cfg(feature = "image")]
pub mod image;
#[cfg(feature = "json")]
pub mod json;
#[cfg(feature = "pdf")]
pub mod pdf;
#[cfg(feature = "powerpoint")]
pub mod powerpoint;
#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(feature = "tar")]
pub mod tar;
#[cfg(feature = "toml_conv")]
pub mod toml_conv;
#[cfg(feature = "video")]
pub mod video;
#[cfg(feature = "word")]
pub mod word;
#[cfg(feature = "xml")]
pub mod xml;
#[cfg(feature = "yaml")]
pub mod yaml;
#[cfg(feature = "zip")]
pub mod zip;

use crate::converter::Converter;
use crate::detect::Format;

pub fn get_converter(format: Format) -> crate::error::Result<Box<dyn Converter>> {
    match format {
        #[cfg(feature = "excel")]
        Format::Excel => Ok(Box::new(excel::ExcelConverter)),
        #[cfg(not(feature = "excel"))]
        Format::Excel => Err(crate::error::Error::FeatureDisabled("excel".into())),

        #[cfg(feature = "pdf")]
        Format::Pdf => Ok(Box::new(pdf::PdfConverter)),
        #[cfg(not(feature = "pdf"))]
        Format::Pdf => Err(crate::error::Error::FeatureDisabled("pdf".into())),

        #[cfg(feature = "powerpoint")]
        Format::PowerPoint => Ok(Box::new(powerpoint::PowerPointConverter)),
        #[cfg(not(feature = "powerpoint"))]
        Format::PowerPoint => Err(crate::error::Error::FeatureDisabled("powerpoint".into())),

        #[cfg(feature = "word")]
        Format::Word => Ok(Box::new(word::WordConverter)),
        #[cfg(not(feature = "word"))]
        Format::Word => Err(crate::error::Error::FeatureDisabled("word".into())),

        #[cfg(feature = "image")]
        Format::Image => Ok(Box::new(image::ImageConverter)),
        #[cfg(not(feature = "image"))]
        Format::Image => Err(crate::error::Error::FeatureDisabled("image".into())),

        #[cfg(feature = "zip")]
        Format::Zip => Ok(Box::new(zip::ZipConverter)),
        #[cfg(not(feature = "zip"))]
        Format::Zip => Err(crate::error::Error::FeatureDisabled("zip".into())),

        #[cfg(feature = "epub")]
        Format::Epub => Ok(Box::new(epub::EpubConverter)),
        #[cfg(not(feature = "epub"))]
        Format::Epub => Err(crate::error::Error::FeatureDisabled("epub".into())),

        #[cfg(feature = "audio")]
        Format::Audio => Ok(Box::new(audio::AudioConverter)),
        #[cfg(not(feature = "audio"))]
        Format::Audio => Err(crate::error::Error::FeatureDisabled("audio".into())),

        #[cfg(feature = "csv")]
        Format::Csv => Ok(Box::new(csv::CsvConverter)),
        #[cfg(not(feature = "csv"))]
        Format::Csv => Err(crate::error::Error::FeatureDisabled("csv".into())),

        #[cfg(feature = "html")]
        Format::Html => Ok(Box::new(html::HtmlConverter)),
        #[cfg(not(feature = "html"))]
        Format::Html => Err(crate::error::Error::FeatureDisabled("html".into())),

        #[cfg(feature = "json")]
        Format::Json => Ok(Box::new(json::JsonConverter)),
        #[cfg(not(feature = "json"))]
        Format::Json => Err(crate::error::Error::FeatureDisabled("json".into())),

        #[cfg(feature = "yaml")]
        Format::Yaml => Ok(Box::new(yaml::YamlConverter)),
        #[cfg(not(feature = "yaml"))]
        Format::Yaml => Err(crate::error::Error::FeatureDisabled("yaml".into())),

        #[cfg(feature = "toml_conv")]
        Format::Toml => Ok(Box::new(toml_conv::TomlConverter)),
        #[cfg(not(feature = "toml_conv"))]
        Format::Toml => Err(crate::error::Error::FeatureDisabled("toml".into())),

        #[cfg(feature = "xml")]
        Format::Xml => Ok(Box::new(xml::XmlConverter)),
        #[cfg(not(feature = "xml"))]
        Format::Xml => Err(crate::error::Error::FeatureDisabled("xml".into())),

        #[cfg(feature = "sqlite")]
        Format::Sqlite => Ok(Box::new(sqlite::SqliteConverter)),
        #[cfg(not(feature = "sqlite"))]
        Format::Sqlite => Err(crate::error::Error::FeatureDisabled("sqlite".into())),

        #[cfg(feature = "tar")]
        Format::Tar => Ok(Box::new(tar::TarConverter)),
        #[cfg(not(feature = "tar"))]
        Format::Tar => Err(crate::error::Error::FeatureDisabled("tar".into())),

        #[cfg(feature = "video")]
        Format::Video => Ok(Box::new(video::VideoConverter)),
        #[cfg(not(feature = "video"))]
        Format::Video => Err(crate::error::Error::FeatureDisabled("video".into())),
    }
}

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
#[cfg(feature = "markdown_docx")]
pub mod markdown_docx;
#[cfg(feature = "markdown_html")]
pub mod markdown_html;
#[cfg(feature = "markdown_text")]
pub mod markdown_text;
#[cfg(feature = "markdown_latex")]
pub mod markdown_latex;
#[cfg(feature = "markdown_rst")]
pub mod markdown_rst;
#[cfg(feature = "markdown_asciidoc")]
pub mod markdown_asciidoc;
#[cfg(feature = "markdown_org")]
pub mod markdown_org;
#[cfg(feature = "markdown_epub_out")]
pub mod markdown_epub_out;
#[cfg(feature = "markdown_json_ast")]
pub mod markdown_json_ast;
#[cfg(feature = "ocr")]
pub mod ocr;
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

        #[cfg(feature = "ocr")]
        Format::Ocr => Ok(Box::new(ocr::OcrConverter)),
        #[cfg(not(feature = "ocr"))]
        Format::Ocr => Err(crate::error::Error::FeatureDisabled("ocr".into())),

        #[cfg(feature = "markdown_docx")]
        Format::MarkdownDocx => Ok(Box::new(markdown_docx::MarkdownDocxConverter)),
        #[cfg(not(feature = "markdown_docx"))]
        Format::MarkdownDocx => Err(crate::error::Error::FeatureDisabled("markdown-docx".into())),

        #[cfg(feature = "markdown_html")]
        Format::MarkdownHtml => Ok(Box::new(markdown_html::MarkdownHtmlConverter)),
        #[cfg(not(feature = "markdown_html"))]
        Format::MarkdownHtml => Err(crate::error::Error::FeatureDisabled("markdown-html".into())),

        #[cfg(feature = "markdown_text")]
        Format::MarkdownText => Ok(Box::new(markdown_text::MarkdownTextConverter)),
        #[cfg(not(feature = "markdown_text"))]
        Format::MarkdownText => Err(crate::error::Error::FeatureDisabled("markdown-text".into())),

        #[cfg(feature = "markdown_latex")]
        Format::MarkdownLatex => Ok(Box::new(markdown_latex::MarkdownLatexConverter)),
        #[cfg(not(feature = "markdown_latex"))]
        Format::MarkdownLatex => Err(crate::error::Error::FeatureDisabled("markdown-latex".into())),

        #[cfg(feature = "markdown_rst")]
        Format::MarkdownRst => Ok(Box::new(markdown_rst::MarkdownRstConverter)),
        #[cfg(not(feature = "markdown_rst"))]
        Format::MarkdownRst => Err(crate::error::Error::FeatureDisabled("markdown-rst".into())),

        #[cfg(feature = "markdown_asciidoc")]
        Format::MarkdownAsciidoc => Ok(Box::new(markdown_asciidoc::MarkdownAsciidocConverter)),
        #[cfg(not(feature = "markdown_asciidoc"))]
        Format::MarkdownAsciidoc => Err(crate::error::Error::FeatureDisabled("markdown-asciidoc".into())),

        #[cfg(feature = "markdown_org")]
        Format::MarkdownOrg => Ok(Box::new(markdown_org::MarkdownOrgConverter)),
        #[cfg(not(feature = "markdown_org"))]
        Format::MarkdownOrg => Err(crate::error::Error::FeatureDisabled("markdown-org".into())),

        #[cfg(feature = "markdown_epub_out")]
        Format::MarkdownEpub => Ok(Box::new(markdown_epub_out::MarkdownEpubConverter)),
        #[cfg(not(feature = "markdown_epub_out"))]
        Format::MarkdownEpub => Err(crate::error::Error::FeatureDisabled("markdown-epub".into())),

        #[cfg(feature = "markdown_json_ast")]
        Format::MarkdownJsonAst => Ok(Box::new(markdown_json_ast::MarkdownJsonAstConverter)),
        #[cfg(not(feature = "markdown_json_ast"))]
        Format::MarkdownJsonAst => Err(crate::error::Error::FeatureDisabled("markdown-json-ast".into())),
    }
}

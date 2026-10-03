//! End-to-end checks of the `mq-conv` binary.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mq-conv-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mq-conv"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn token_budget_rejects_binary_docx_output() {
    let dir = scratch("docx-budget");
    std::fs::write(dir.join("report.md"), "# Title\n\nSome text.\n").unwrap();

    // Without a budget the DOCX bytes are written verbatim.
    let ok = run(&dir, &["report.md", "--to", "docx"]);
    assert!(ok.status.success());
    assert!(
        ok.stdout.starts_with(b"PK"),
        "not a zip: {:?}",
        &ok.stdout[..8]
    );

    // With one, the CLI refuses instead of emitting lossy UTF-8.
    let budgeted = run(&dir, &["report.md", "--to", "docx", "--max-tokens", "4000"]);
    assert!(!budgeted.status.success());
    let stderr = String::from_utf8_lossy(&budgeted.stderr);
    assert!(stderr.contains("binary"), "{stderr}");

    let cursor = run(&dir, &["report.md", "--to", "docx", "--cursor", "1"]);
    assert!(!cursor.status.success());
    std::fs::remove_dir_all(&dir).unwrap();
}

fn docx_with_image() -> Vec<u8> {
    let document = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>
<w:p><w:r><w:t>Hello</w:t></w:r></w:p>
<w:p><w:r><w:drawing><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:blipFill><a:blip r:embed="rId5"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></w:drawing></w:r></w:p>
</w:body></w:document>"#;
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId5" Type="image" Target="media/image1.png"/></Relationships>"#;

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/media/image1.png", b"PNGDATA"),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn output_dir_markdown_links_images_relative_to_itself() {
    let dir = scratch("image-links");
    std::fs::write(dir.join("report.docx"), docx_with_image()).unwrap();

    let out = run(
        &dir,
        &[
            "report.docx",
            "--output-dir",
            "out",
            "--extract-images",
            "img",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Images are saved next to `out`, so the Markdown in `out` must climb up.
    let md = std::fs::read_to_string(dir.join("out/report.md")).unwrap();
    assert!(md.contains("![image](../img/image1.png)"), "{md}");
    assert_eq!(
        std::fs::read(dir.join("img/image1.png")).unwrap(),
        b"PNGDATA"
    );
    assert!(dir.join("out").join("../img/image1.png").exists());

    // Printed to stdout the links stay relative to the working directory.
    let stdout = run(&dir, &["report.docx", "--extract-images", "img"]);
    let md = String::from_utf8_lossy(&stdout.stdout);
    assert!(md.contains("![image](img/image1.png)"), "{md}");
    std::fs::remove_dir_all(&dir).unwrap();
}

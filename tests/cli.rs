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

const HTML: &str = "<h1>Guide</h1><p>Intro text.</p><h2>Usage</h2><p>Run it.</p>\
<h2>Install</h2><p>Cargo.</p>";

fn html_dir(name: &str) -> PathBuf {
    let dir = scratch(name);
    std::fs::write(dir.join("guide.html"), HTML).unwrap();
    dir
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn query_prints_only_the_matching_nodes() {
    let dir = html_dir("query");
    let all = run(&dir, &["guide.html"]);
    assert!(stdout(&all).contains("Intro text."));

    let out = run(&dir, &["guide.html", "-q", ".h2"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "## Usage\n\n## Install\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn query_functions_know_the_input() {
    let dir = html_dir("query-fns");
    // The query runs once per top-level node, so the line repeats.
    let out = run(&dir, &["guide.html", "-q", r#"format() + " " + filename()"#]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(!text.is_empty());
    assert!(text.lines().all(|l| l == "html guide.html"), "{text}");

    let out = run(&dir, &["guide.html", "-q", ".h2 | select(tokens(.) > 100)"]);
    assert_eq!(stdout(&out), "");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn query_runs_per_file_and_before_the_token_budget() {
    let dir = html_dir("query-multi");
    std::fs::write(dir.join("other.html"), "<h1>Other</h1>").unwrap();
    let out = run(&dir, &["guide.html", "other.html", "-q", ".h1"]);
    assert_eq!(stdout(&out), "# Guide\n\n---\n\n# Other\n");

    let out = run(&dir, &["guide.html", "-q", ".h2", "--max-tokens", "3"]);
    let text = stdout(&out);
    assert!(text.starts_with("## Usage"), "{text}");
    assert!(text.contains("next-cursor="), "{text}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn query_errors_are_reported() {
    let dir = html_dir("query-err");
    let bad = run(&dir, &["guide.html", "-q", "select("]);
    assert!(!bad.status.success());

    std::fs::write(dir.join("notes.md"), "# Notes\n").unwrap();
    let with_to = run(&dir, &["notes.md", "--to", "html", "-q", ".h1"]);
    assert!(stderr(&with_to).contains("--to"), "{}", stderr(&with_to));
    let md_input = run(&dir, &["notes.md", "-q", ".h1"]);
    assert!(!md_input.status.success());
    assert!(stderr(&md_input).contains("Markdown output"), "{}", stderr(&md_input));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn pages_is_rejected_for_non_pdf_input() {
    let dir = html_dir("pages");
    let out = run(&dir, &["guide.html", "--pages", "1"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("PDF"), "{}", stderr(&out));

    let bad = run(&dir, &["guide.html", "--pages", "3-1"]);
    assert!(!bad.status.success());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn pages_selects_pdf_pages_through_the_cli() {
    let dir = scratch("pdf-pages");
    let pdf = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/pdf/sample_ja.pdf"))
        .unwrap();
    std::fs::write(dir.join("a.pdf"), pdf).unwrap();

    let first = run(&dir, &["a.pdf", "--pages", "1"]);
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(stdout(&first).contains("<!-- page 1 -->"));

    let none = run(&dir, &["a.pdf", "--pages", "50"]);
    assert!(!none.status.success());
    assert!(stderr(&none).contains("no page matches"), "{}", stderr(&none));
    std::fs::remove_dir_all(&dir).unwrap();
}

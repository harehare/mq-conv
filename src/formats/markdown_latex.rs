use std::io::Write;

use mq_markdown::{Markdown, Node};

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct MarkdownLatexConverter;

impl Converter for MarkdownLatexConverter {
    fn format_name(&self) -> &'static str {
        "markdown-latex"
    }

    fn output_extension(&self) -> &'static str {
        "tex"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let markdown = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "markdown-latex",
            message: format!("Input is not valid UTF-8: {e}"),
        })?;

        let parsed = markdown.parse::<Markdown>().map_err(|e| Error::Conversion {
            format: "markdown-latex",
            message: e.to_string(),
        })?;

        write_latex(&parsed.nodes, writer).map_err(|e| Error::Conversion {
            format: "markdown-latex",
            message: e.to_string(),
        })?;

        Ok(())
    }
}

fn latex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str(r"\&"),
            '%' => out.push_str(r"\%"),
            '$' => out.push_str(r"\$"),
            '#' => out.push_str(r"\#"),
            '_' => out.push_str(r"\_"),
            '{' => out.push_str(r"\{"),
            '}' => out.push_str(r"\}"),
            '~' => out.push_str(r"\textasciitilde{}"),
            '^' => out.push_str(r"\textasciicircum{}"),
            '\\' => out.push_str(r"\textbackslash{}"),
            c => out.push(c),
        }
    }
    out
}

fn inline_to_latex(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(&latex_escape(&t.value)),
            Node::Strong(s) => {
                out.push_str(r"\textbf{");
                out.push_str(&inline_to_latex(&s.values));
                out.push('}');
            }
            Node::Emphasis(e) => {
                out.push_str(r"\textit{");
                out.push_str(&inline_to_latex(&e.values));
                out.push('}');
            }
            Node::CodeInline(c) => {
                out.push_str(r"\texttt{");
                out.push_str(&latex_escape(&c.value));
                out.push('}');
            }
            Node::Link(l) => {
                let url = l.url.as_str();
                let text = inline_to_latex(&l.values);
                out.push_str(&format!(r"\href{{{url}}}{{{text}}}"));
            }
            Node::Delete(d) => {
                out.push_str(r"\sout{");
                out.push_str(&inline_to_latex(&d.values));
                out.push('}');
            }
            Node::Break(_) => out.push(' '),
            _ => {}
        }
    }
    out
}

fn write_latex(nodes: &[Node], writer: &mut dyn Write) -> std::io::Result<()> {
    writeln!(writer, r"\documentclass{{article}}")?;
    writeln!(writer, r"\usepackage[utf8]{{inputenc}}")?;
    writeln!(writer, r"\usepackage{{hyperref}}")?;
    writeln!(writer, r"\usepackage{{ulem}}")?;
    writeln!(writer, r"\usepackage{{listings}}")?;
    writeln!(writer, r"\usepackage{{graphicx}}")?;
    writeln!(writer, r"\begin{{document}}")?;
    writeln!(writer)?;

    let mut table_data: Vec<(usize, usize, String)> = Vec::new();
    let mut list_env: Option<&str> = None;

    macro_rules! close_list {
        () => {
            if let Some(env) = list_env.take() {
                writeln!(writer, r"\end{{{env}}}")?;
                writeln!(writer)?;
            }
        };
    }

    macro_rules! flush_table {
        () => {
            if !table_data.is_empty() {
                let max_col = table_data.iter().map(|(_, c, _)| *c).max().unwrap_or(0) + 1;
                let max_row = table_data.iter().map(|(r, _, _)| *r).max().unwrap_or(0);
                let col_spec: String = "l ".repeat(max_col);
                writeln!(writer, r"\begin{{tabular}}{{{}}}", col_spec.trim())?;
                writeln!(writer, r"\hline")?;
                for row_idx in 0..=max_row {
                    let mut cells: Vec<String> = Vec::new();
                    for col_idx in 0..max_col {
                        let text = table_data
                            .iter()
                            .find(|(r, c, _)| *r == row_idx && *c == col_idx)
                            .map(|(_, _, t)| t.as_str())
                            .unwrap_or("");
                        if row_idx == 0 {
                            cells.push(format!(r"\textbf{{{text}}}"));
                        } else {
                            cells.push(text.to_string());
                        }
                    }
                    writeln!(writer, "{} \\\\", cells.join(" & "))?;
                    if row_idx == 0 {
                        writeln!(writer, r"\hline")?;
                    }
                }
                writeln!(writer, r"\hline")?;
                writeln!(writer, r"\end{{tabular}}")?;
                writeln!(writer)?;
                table_data.clear();
            }
        };
    }

    for node in nodes {
        match node {
            Node::TableCell(cell) => {
                close_list!();
                let text = extract_text(&cell.values);
                table_data.push((cell.row, cell.column, text));
                continue;
            }
            Node::TableAlign(_) => continue,
            _ => {
                flush_table!();
            }
        }

        match node {
            Node::Heading(h) => {
                close_list!();
                let text = inline_to_latex(&h.values);
                let cmd = match h.depth {
                    1 => r"\section",
                    2 => r"\subsection",
                    3 => r"\subsubsection",
                    _ => r"\paragraph",
                };
                writeln!(writer, "{cmd}{{{text}}}")?;
                writeln!(writer)?;
            }

            Node::Code(c) => {
                close_list!();
                if let Some(lang) = &c.lang {
                    writeln!(writer, r"\begin{{lstlisting}}[language={lang}]")?;
                } else {
                    writeln!(writer, r"\begin{{verbatim}}")?;
                }
                writeln!(writer, "{}", c.value)?;
                if c.lang.is_some() {
                    writeln!(writer, r"\end{{lstlisting}}")?;
                } else {
                    writeln!(writer, r"\end{{verbatim}}")?;
                }
                writeln!(writer)?;
            }

            Node::List(l) => {
                let env = if l.ordered { "enumerate" } else { "itemize" };
                if list_env != Some(env) {
                    close_list!();
                    writeln!(writer, r"\begin{{{env}}}")?;
                    list_env = Some(env);
                }
                let text = inline_to_latex(&l.values);
                writeln!(writer, r"\item {text}")?;
            }

            Node::Blockquote(bq) => {
                close_list!();
                let text = inline_to_latex(&bq.values);
                writeln!(writer, r"\begin{{quote}}")?;
                writeln!(writer, "{text}")?;
                writeln!(writer, r"\end{{quote}}")?;
                writeln!(writer)?;
            }

            Node::HorizontalRule(_) => {
                close_list!();
                writeln!(writer, r"\noindent\rule{{\linewidth}}{{0.4pt}}")?;
                writeln!(writer)?;
            }

            Node::Text(_)
            | Node::Strong(_)
            | Node::Emphasis(_)
            | Node::CodeInline(_)
            | Node::Break(_)
            | Node::Link(_)
            | Node::Delete(_) => {
                close_list!();
                let text = inline_to_latex(std::slice::from_ref(node));
                writeln!(writer, "{text}")?;
                writeln!(writer)?;
            }

            _ => {}
        }
    }

    flush_table!();
    if let Some(env) = list_env {
        writeln!(writer, r"\end{{{env}}}")?;
        writeln!(writer)?;
    }

    writeln!(writer, r"\end{{document}}")?;
    Ok(())
}

fn extract_text(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(&t.value),
            Node::Strong(s) => out.push_str(&extract_text(&s.values)),
            Node::Emphasis(e) => out.push_str(&extract_text(&e.values)),
            Node::CodeInline(c) => out.push_str(&c.value),
            Node::Link(l) => out.push_str(&extract_text(&l.values)),
            Node::Delete(d) => out.push_str(&extract_text(&d.values)),
            Node::Break(_) => out.push(' '),
            _ => {}
        }
    }
    out
}

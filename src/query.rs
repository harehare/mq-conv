//! Run [mq](https://mqlang.org) queries over converted Markdown.
//!
//! Besides the standard mq functions, queries can call a few functions that
//! know about the conversion:
//!
//! - `tokens(x)`: estimated token count of a node or string, the same
//!   estimate `--max-tokens` uses
//! - `format()`: the detected input format (`"pdf"`, `"word"`, …)
//! - `filename()`: the input file name, or `None` when reading stdin

use mq_lang::{DefaultEngine, HostFnResult, HostFunctionError, RuntimeValue, RuntimeValues};
use mq_markdown::Markdown;

use crate::budget::estimate_tokens;

/// What the query can ask about the input it runs on.
#[derive(Debug, Clone, Default)]
pub struct QueryContext {
    pub format: Option<String>,
    pub filename: Option<String>,
}

/// Run `query` over `markdown` and render the result as Markdown.
pub fn run(query: &str, markdown: &str, ctx: &QueryContext) -> miette::Result<String> {
    let mut engine = DefaultEngine::default();
    engine.load_builtin_module();
    register_functions(&engine, ctx);

    let input = mq_lang::parse_markdown_input(markdown)?;
    let values = engine
        .eval(query, input.into_iter())
        .map_err(|e| miette::Report::new(*e))?;

    let nodes = RuntimeValues::from(values.compact()).into_markdown_nodes();
    let mut out = Markdown::new(nodes).to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn register_functions(engine: &DefaultEngine, ctx: &QueryContext) {
    engine.register_fn("tokens", |args: &[RuntimeValue]| -> HostFnResult {
        match args {
            [value] => {
                let text = match value.markdown_node() {
                    Some(node) => node.to_string(),
                    None => value.to_string(),
                };
                Ok(RuntimeValue::from(estimate_tokens(&text)))
            }
            _ => Err(HostFunctionError::new("tokens() expects one argument")),
        }
    });
    register_constant(engine, "format", ctx.format.clone());
    register_constant(engine, "filename", ctx.filename.clone());
}

/// A no-argument function returning a fixed string, or `None` when unknown.
fn register_constant(engine: &DefaultEngine, name: &'static str, value: Option<String>) {
    engine.register_fn(name, move |args: &[RuntimeValue]| -> HostFnResult {
        if !args.is_empty() {
            return Err(HostFunctionError::new(format!(
                "{name}() takes no arguments"
            )));
        }
        Ok(value.clone().map_or(RuntimeValue::NONE, RuntimeValue::from))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Title\n\nIntro text.\n\n## Usage\n\nRun it.\n\n## Install\n\nCargo.\n";

    fn query(q: &str) -> String {
        run(q, DOC, &QueryContext::default()).unwrap()
    }

    #[test]
    fn selects_nodes_and_renders_markdown() {
        assert_eq!(query(".h2"), "## Usage\n\n## Install\n");
        assert_eq!(query(".h1"), "# Title\n");
    }

    #[test]
    fn no_match_gives_empty_output() {
        assert_eq!(query(".code"), "");
    }

    #[test]
    fn invalid_query_is_an_error() {
        assert!(run("select(", DOC, &QueryContext::default()).is_err());
        assert!(run("nope(1)", DOC, &QueryContext::default()).is_err());
    }

    #[test]
    fn tokens_matches_the_budget_estimate() {
        let n = estimate_tokens("## Usage");
        assert_eq!(
            query(r#".h2 | select(tokens(.) == 2) | to_text()"#),
            "Usage\n"
        );
        // A query runs once per top-level node.
        let per_node = query("tokens(\"abcdefgh\") | to_text()");
        assert!(per_node.lines().all(|l| l == "2"), "{per_node}");
        assert_eq!(n, 2);
    }

    #[test]
    fn tokens_can_filter_long_sections() {
        let doc = format!("# A\n\nshort\n\n# B\n\n{}\n", "word ".repeat(100));
        let out = run(
            r#".h1 | select(tokens(.) < 5)"#,
            &doc,
            &QueryContext::default(),
        )
        .unwrap();
        assert_eq!(out, "# A\n\n# B\n");
    }

    #[test]
    fn input_facts_are_available() {
        let ctx = QueryContext {
            format: Some("pdf".into()),
            filename: Some("report.pdf".into()),
        };
        let out = run(r#"format() + " " + filename()"#, DOC, &ctx).unwrap();
        assert!(out.contains("pdf report.pdf"), "{out}");

        // Unknown input (stdin) has no file name.
        let none = run("filename()", DOC, &QueryContext::default()).unwrap();
        assert_eq!(none, "");
    }

    #[test]
    fn wrong_arity_is_reported() {
        assert!(run("tokens()", DOC, &QueryContext::default()).is_err());
        assert!(run("format(1)", DOC, &QueryContext::default()).is_err());
    }
}

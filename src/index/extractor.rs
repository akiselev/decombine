use std::ops::Range;

use anyhow::{Context, Result};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, QueryCursor};

use crate::db::NewCodeUnit;
use crate::index::language::{LanguageDef, PendingUnit};
use crate::index::normalizer::{merge_ranges, normalize_for_hash, sha256_hex, strip_ranges};

#[derive(Debug, Clone, Copy)]
pub struct ExtractOptions {
    /// Units with fewer named AST nodes in their body are dropped.
    pub body_node_count_threshold: usize,
    /// Units whose stripped embedding text exceeds this length are dropped.
    pub max_body_chars: usize,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            body_node_count_threshold: 10,
            max_body_chars: 10_000,
        }
    }
}

/// Parse `source` and extract code units using the language's bundled query
/// and adapter. Returned units always carry display/embedding text; the
/// indexer applies the retention mode before storage.
pub fn extract_units(
    def: &LanguageDef,
    source: &str,
    options: &ExtractOptions,
) -> Result<Vec<NewCodeUnit>> {
    let mut parser = Parser::new();
    parser
        .set_language(&def.language)
        .with_context(|| format!("loading grammar for {}", def.spec.id))?;
    let tree = parser
        .parse(source, None)
        .with_context(|| format!("parsing {} source", def.spec.id))?;

    let unit_idx = capture_index(def, "unit");
    let name_idx = capture_index(def, "unit.name");
    let body_idx = capture_index(def, "unit.body");
    let strip_idx = capture_index(def, "unit.strip");
    let scope_idx = capture_index(def, "unit.scope");

    let mut units = Vec::new();
    let mut seen_ranges: Vec<Range<usize>> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&def.query, tree.root_node(), source.as_bytes());
    while let Some(query_match) = matches.next() {
        let mut unit_node: Option<Node> = None;
        let mut pending = PendingUnit {
            node: tree.root_node(),
            body: None,
            kind: "function".to_string(),
            name: None,
            scope: None,
            strip: Vec::new(),
        };
        for capture in query_match.captures {
            let index = Some(capture.index);
            if index == unit_idx {
                unit_node = Some(capture.node);
            } else if index == name_idx {
                pending.name = Some(source[capture.node.byte_range()].to_string());
            } else if index == body_idx {
                pending.body = Some(capture.node);
            } else if index == strip_idx {
                pending.strip.push(capture.node.byte_range());
            } else if index == scope_idx {
                pending.scope = Some(source[capture.node.byte_range()].to_string());
            }
        }
        let Some(node) = unit_node else { continue };
        pending.node = node;

        // `#set! unit.kind "..."` on the matched pattern.
        for property in def.query.property_settings(query_match.pattern_index) {
            if &*property.key == "unit.kind"
                && let Some(value) = &property.value
            {
                pending.kind = value.to_string();
            }
        }

        if let Some(adapter) = def.adapter {
            adapter.refine(source, &mut pending);
        }

        // The same node can match several patterns; keep the first match.
        let range = pending.node.byte_range();
        if seen_ranges.contains(&range) {
            continue;
        }

        if let Some(unit) = build_unit(def, source, pending, options) {
            seen_ranges.push(range);
            units.push(unit);
        }
    }
    units.sort_by_key(|u| (u.start_byte, u.end_byte));
    Ok(units)
}

fn capture_index(def: &LanguageDef, name: &str) -> Option<u32> {
    def.query
        .capture_names()
        .iter()
        .position(|n| *n == name)
        .map(|i| i as u32)
}

fn build_unit(
    def: &LanguageDef,
    source: &str,
    pending: PendingUnit<'_>,
    options: &ExtractOptions,
) -> Option<NewCodeUnit> {
    let node = pending.node;
    let start_byte = node.start_byte();
    let end_byte = node.end_byte();
    let display_source = &source[start_byte..end_byte];

    // Complexity gate: named nodes under the body (or the whole unit).
    let body_node = pending.body.unwrap_or(node);
    let body_node_count = count_named_nodes(body_node);
    if body_node_count < options.body_node_count_threshold {
        return None;
    }

    // Strip ranges: explicit @unit.strip captures, adapter additions, and
    // all comment nodes inside the unit. Made relative to the unit start.
    let mut strip: Vec<Range<usize>> = pending
        .strip
        .iter()
        .map(|r| r.start.saturating_sub(start_byte)..r.end.saturating_sub(start_byte))
        .collect();
    collect_comment_ranges(node, &def.spec.comment_nodes, start_byte, &mut strip);
    let strip = merge_ranges(&strip);

    let embedding_text = strip_ranges(display_source, &strip);
    let normalized = normalize_for_hash(&embedding_text);
    if normalized.is_empty() || embedding_text.chars().count() > options.max_body_chars {
        return None;
    }

    let scope = pending
        .scope
        .clone()
        .or_else(|| recover_scope(def, source, node));

    Some(NewCodeUnit {
        language_id: def.spec.id.clone(),
        kind: pending.kind,
        name: pending.name.unwrap_or_else(|| "<anonymous>".to_string()),
        scope,
        start_byte,
        end_byte,
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        body_node_count,
        source_hash: sha256_hex(display_source),
        normalized_body_hash: sha256_hex(&normalized),
        display_source: Some(display_source.to_string()),
        embedding_text: Some(embedding_text),
    })
}

fn count_named_nodes(node: Node<'_>) -> usize {
    let mut count: usize = 0;
    let mut cursor = node.walk();
    let mut done = false;
    while !done {
        if cursor.node().is_named() {
            count += 1;
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() || cursor.node() == node {
                done = true;
                break;
            }
        }
    }
    // The walk counts `node` itself; body complexity means its contents.
    count.saturating_sub(1)
}

fn collect_comment_ranges(
    node: Node<'_>,
    comment_kinds: &[String],
    unit_start: usize,
    out: &mut Vec<Range<usize>>,
) {
    if comment_kinds.is_empty() {
        return;
    }
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if comment_kinds.iter().any(|k| k == current.kind()) {
            let range = current.byte_range();
            out.push(range.start - unit_start..range.end - unit_start);
            continue;
        }
        for i in 0..current.child_count() as u32 {
            if let Some(child) = current.child(i) {
                stack.push(child);
            }
        }
    }
}

/// Walk ancestors and build a display scope like `Outer.Inner` from the
/// spec's scope rules (classes, impls, modules, ...).
fn recover_scope(def: &LanguageDef, source: &str, node: Node<'_>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = node.parent();
    while let Some(ancestor) = current {
        for rule in &def.spec.scopes {
            if ancestor.kind() == rule.kind
                && let Some(name) = ancestor.child_by_field_name(rule.field.as_str())
            {
                parts.push(source[name.byte_range()].to_string());
            }
        }
        current = ancestor.parent();
    }
    if parts.is_empty() {
        None
    } else {
        parts.reverse();
        Some(parts.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::language::LanguageRegistry;

    fn extract(language: &str, source: &str, threshold: usize) -> Vec<NewCodeUnit> {
        let def = LanguageRegistry::global().get(language).unwrap();
        extract_units(
            def,
            source,
            &ExtractOptions {
                body_node_count_threshold: threshold,
                max_body_chars: 10_000,
            },
        )
        .unwrap()
    }

    #[test]
    fn rust_functions_methods_and_comments() {
        let source = r#"
mod outer {
    impl Widget {
        /// Doc comment.
        fn render(&self, x: i32) -> i32 {
            // inline comment
            let y = x + 1;
            let z = y * 2;
            z - x
        }
    }
}

fn tiny() { 1 }
"#;
        let units = extract("rust", source, 5);
        assert_eq!(units.len(), 1, "tiny() is below the threshold");
        let unit = &units[0];
        assert_eq!(unit.name, "render");
        assert_eq!(unit.kind, "function");
        assert_eq!(unit.scope.as_deref(), Some("outer.Widget"));
        assert!(unit.embedding_text.as_ref().unwrap().contains("let y"));
        assert!(
            !unit
                .embedding_text
                .as_ref()
                .unwrap()
                .contains("inline comment")
        );
        // Doc comment above the fn is outside the unit range entirely.
        assert!(
            !unit
                .display_source
                .as_ref()
                .unwrap()
                .contains("Doc comment")
        );
        assert_eq!(unit.start_line, 5);
        assert_eq!(unit.end_line, 10);
    }

    #[test]
    fn comment_only_difference_hashes_equal() {
        let a = extract(
            "rust",
            "fn f(a: i32) -> i32 { let b = a + 1; let c = b * 2; c }",
            3,
        );
        let b = extract(
            "rust",
            "fn f(a: i32) -> i32 {\n  // different comment\n  let b = a + 1;\n  let c = b * 2;\n  c\n}",
            3,
        );
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0].normalized_body_hash, b[0].normalized_body_hash);
        assert_ne!(a[0].source_hash, b[0].source_hash);
    }

    #[test]
    fn python_docstring_stripped_and_class_scope() {
        let source = r#"
class Greeter:
    def greet(self, name):
        """Say hello.

        Longer docstring text.
        """
        message = "hello " + name
        print(message)
        return message
"#;
        let units = extract("python", source, 3);
        assert_eq!(units.len(), 1);
        let unit = &units[0];
        assert_eq!(unit.name, "greet");
        assert_eq!(unit.scope.as_deref(), Some("Greeter"));
        assert!(!unit.embedding_text.as_ref().unwrap().contains("Say hello"));
        assert!(unit.display_source.as_ref().unwrap().contains("Say hello"));
    }

    #[test]
    fn javascript_anonymous_functions_named_from_context() {
        let source = r#"
const handler = (req, res) => {
    const body = parse(req);
    const result = transform(body);
    res.send(result);
};

registry.callback = function (event) {
    const a = event.x + event.y;
    return a * 2;
};
"#;
        let units = extract("javascript", source, 3);
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"handler"), "names: {names:?}");
        assert!(names.contains(&"registry.callback"), "names: {names:?}");
        assert!(units.iter().all(|u| u.kind == "closure"));
    }

    #[test]
    fn nested_units_both_extracted() {
        let source = r#"
fn outer(items: Vec<i32>) -> Vec<i32> {
    let doubled: Vec<i32> = items.iter().map(|x| {
        let a = x + 1;
        let b = a * 2;
        b - 1
    }).collect();
    doubled
}
"#;
        let units = extract("rust", source, 3);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].kind, "function");
        assert_eq!(units[1].kind, "closure");
        // The closure is inside the function's byte range.
        assert!(units[1].start_byte > units[0].start_byte);
        assert!(units[1].end_byte < units[0].end_byte);
    }

    #[test]
    fn go_method_receiver_scope() {
        let source = r#"
package main

func (s *Server) Handle(w http.ResponseWriter, r *http.Request) {
    body := readAll(r)
    result := process(body)
    w.Write(result)
}
"#;
        let units = extract("go", source, 3);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "Handle");
        assert_eq!(units[0].kind, "method");
        assert_eq!(units[0].scope.as_deref(), Some("Server"));
    }

    #[test]
    fn small_bodies_filtered() {
        let units = extract("python", "def one():\n    return 1\n", 10);
        assert!(units.is_empty());
    }
}

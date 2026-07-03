use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use serde::Deserialize;
use tree_sitter::{Language, Node, Query};

/// Parsed `assets/languages/<id>.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageSpec {
    pub id: String,
    pub name: String,
    pub extensions: Vec<String>,
    /// Node kinds stripped from unit text before hashing/embedding.
    #[serde(default)]
    pub comment_nodes: Vec<String>,
    /// Optional adapter hook name (see `adapter_by_name`).
    #[serde(default)]
    pub adapter: Option<String>,
    /// Ancestor node kinds that contribute display scope, e.g. classes.
    #[serde(default)]
    pub scopes: Vec<ScopeRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeRule {
    pub kind: String,
    pub field: String,
}

/// A unit in flight between query matching and final `NewCodeUnit`
/// construction. Adapters may refine any of these fields.
pub struct PendingUnit<'t> {
    pub node: Node<'t>,
    pub body: Option<Node<'t>>,
    pub kind: String,
    pub name: Option<String>,
    pub scope: Option<String>,
    /// Absolute byte ranges (into the file source) removed before
    /// normalization, in addition to comment nodes.
    pub strip: Vec<Range<usize>>,
}

/// Hook layer for cases queries cannot express cleanly: anonymous-function
/// naming, receiver scopes, docstrings, decorators, macros, preprocessors.
pub trait LanguageAdapter: Send + Sync {
    fn refine(&self, source: &str, unit: &mut PendingUnit<'_>);
}

/// Python: strip a leading docstring from function bodies.
struct PythonAdapter;

impl LanguageAdapter for PythonAdapter {
    fn refine(&self, _source: &str, unit: &mut PendingUnit<'_>) {
        let Some(body) = unit.body else { return };
        let Some(first) = body.named_child(0) else {
            return;
        };
        if first.kind() == "expression_statement"
            && let Some(expr) = first.named_child(0)
            && expr.kind() == "string"
        {
            unit.strip.push(first.byte_range());
        }
    }
}

/// JavaScript/TypeScript: name anonymous functions from their assignment
/// context (`const f = () => ...`, `{ f: function() {} }`, `x.f = ...`).
struct JsLikeAdapter;

impl LanguageAdapter for JsLikeAdapter {
    fn refine(&self, source: &str, unit: &mut PendingUnit<'_>) {
        if unit.name.is_some() {
            return;
        }
        let mut node = unit.node;
        // Skip wrappers like parenthesized expressions.
        while let Some(parent) = node.parent() {
            let name = match parent.kind() {
                "variable_declarator" => parent
                    .child_by_field_name("name")
                    .map(|n| source[n.byte_range()].to_string()),
                "pair" => parent
                    .child_by_field_name("key")
                    .map(|n| source[n.byte_range()].to_string()),
                "assignment_expression" => parent
                    .child_by_field_name("left")
                    .map(|n| source[n.byte_range()].to_string()),
                "parenthesized_expression" => {
                    node = parent;
                    continue;
                }
                _ => None,
            };
            if name.is_some() {
                unit.name = name;
            }
            break;
        }
    }
}

/// Go: use the receiver type as the method scope.
struct GoAdapter;

impl LanguageAdapter for GoAdapter {
    fn refine(&self, source: &str, unit: &mut PendingUnit<'_>) {
        if unit.node.kind() != "method_declaration" || unit.scope.is_some() {
            return;
        }
        if let Some(receiver) = unit.node.child_by_field_name("receiver") {
            // receiver is a parameter_list like `(s *Server)`; scope is the
            // type with pointer/generic sigils stripped.
            let text = source[receiver.byte_range()]
                .trim_matches(|c| c == '(' || c == ')')
                .split_whitespace()
                .last()
                .map(|t| t.trim_start_matches(['*', '&']).to_string());
            unit.scope = text;
        }
    }
}

pub fn adapter_by_name(name: &str) -> Result<&'static dyn LanguageAdapter> {
    static PYTHON: PythonAdapter = PythonAdapter;
    static JS_LIKE: JsLikeAdapter = JsLikeAdapter;
    static GO: GoAdapter = GoAdapter;
    match name {
        "python" => Ok(&PYTHON),
        "js-like" => Ok(&JS_LIKE),
        "go" => Ok(&GO),
        other => anyhow::bail!("unknown language adapter {other:?}"),
    }
}

/// One bundled language: its spec, grammar, compiled query, and adapter.
pub struct LanguageDef {
    pub spec: LanguageSpec,
    pub language: Language,
    pub query: Query,
    pub adapter: Option<&'static dyn LanguageAdapter>,
}

macro_rules! bundled {
    ($id:literal, $lang:expr) => {
        (
            $id,
            include_str!(concat!("../../assets/languages/", $id, ".toml")),
            include_str!(concat!("../../assets/languages/", $id, "/units.scm")),
            Language::new($lang),
        )
    };
}

fn bundled_languages() -> Vec<(&'static str, &'static str, &'static str, Language)> {
    vec![
        bundled!("rust", tree_sitter_rust::LANGUAGE),
        bundled!("python", tree_sitter_python::LANGUAGE),
        bundled!("javascript", tree_sitter_javascript::LANGUAGE),
        bundled!("typescript", tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
        bundled!("java", tree_sitter_java::LANGUAGE),
        bundled!("kotlin", tree_sitter_kotlin_ng::LANGUAGE),
        bundled!("csharp", tree_sitter_c_sharp::LANGUAGE),
        bundled!("go", tree_sitter_go::LANGUAGE),
    ]
}

/// All bundled languages keyed by id, loaded once.
pub struct LanguageRegistry {
    languages: BTreeMap<String, LanguageDef>,
}

impl LanguageRegistry {
    fn load() -> Result<Self> {
        let mut languages = BTreeMap::new();
        for (id, spec_toml, query_src, language) in bundled_languages() {
            let spec: LanguageSpec = toml::from_str(spec_toml)
                .with_context(|| format!("parsing language spec for {id}"))?;
            anyhow::ensure!(spec.id == id, "language spec id mismatch for {id}");
            let query = Query::new(&language, query_src)
                .with_context(|| format!("compiling units.scm for {id}"))?;
            let adapter = spec
                .adapter
                .as_deref()
                .map(adapter_by_name)
                .transpose()
                .with_context(|| format!("resolving adapter for {id}"))?;
            languages.insert(
                id.to_string(),
                LanguageDef {
                    spec,
                    language,
                    query,
                    adapter,
                },
            );
        }
        Ok(Self { languages })
    }

    /// The process-wide registry. Compiling queries is cheap enough to do
    /// once at first use; a failure here is a packaging bug.
    pub fn global() -> &'static LanguageRegistry {
        static REGISTRY: OnceLock<LanguageRegistry> = OnceLock::new();
        REGISTRY
            .get_or_init(|| LanguageRegistry::load().expect("bundled language assets must compile"))
    }

    pub fn get(&self, id: &str) -> Option<&LanguageDef> {
        self.languages.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.languages.keys().map(|s| s.as_str())
    }

    /// Resolve a language id from a file extension (lowercase, no dot).
    pub fn by_extension(&self, extension: &str) -> Option<&LanguageDef> {
        self.languages
            .values()
            .find(|def| def.spec.extensions.iter().any(|e| e == extension))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_bundled_languages_load() {
        let registry = LanguageRegistry::global();
        let ids: Vec<&str> = registry.ids().collect();
        assert_eq!(
            ids,
            vec![
                "csharp",
                "go",
                "java",
                "javascript",
                "kotlin",
                "python",
                "rust",
                "typescript"
            ]
        );
    }

    #[test]
    fn registry_matches_config_language_ids() {
        let registry = LanguageRegistry::global();
        for id in crate::config::KNOWN_LANGUAGE_IDS {
            assert!(registry.get(id).is_some(), "config id {id} missing");
        }
        assert_eq!(
            registry.ids().count(),
            crate::config::KNOWN_LANGUAGE_IDS.len()
        );
    }

    #[test]
    fn extension_resolution() {
        let registry = LanguageRegistry::global();
        assert_eq!(registry.by_extension("rs").unwrap().spec.id, "rust");
        assert_eq!(registry.by_extension("tsx").unwrap().spec.id, "typescript");
        assert_eq!(registry.by_extension("mjs").unwrap().spec.id, "javascript");
        assert!(registry.by_extension("txt").is_none());
    }
}

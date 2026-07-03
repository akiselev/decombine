//! Golden extraction tests: each fixture source file has a `.expected`
//! snapshot describing the extracted units. Regenerate snapshots with
//! `UPDATE_GOLDEN=1 cargo test --test extraction_golden`.

use std::fmt::Write as _;
use std::path::Path;

use decombine::index::extractor::{ExtractOptions, extract_units};
use decombine::index::language::LanguageRegistry;

fn snapshot(source_path: &Path) -> String {
    let source = std::fs::read_to_string(source_path).unwrap();
    let extension = source_path.extension().unwrap().to_str().unwrap();
    let def = LanguageRegistry::global()
        .by_extension(extension)
        .unwrap_or_else(|| panic!("no language for extension {extension}"));
    let units = extract_units(
        def,
        &source,
        &ExtractOptions {
            body_node_count_threshold: 8,
            max_body_chars: 10_000,
        },
    )
    .unwrap();

    let mut out = String::new();
    for unit in &units {
        writeln!(
            out,
            "{kind} {name} scope={scope} lines={sl}-{el} nodes={nodes} hash={hash}",
            kind = unit.kind,
            name = unit.name,
            scope = unit.scope.as_deref().unwrap_or("-"),
            sl = unit.start_line,
            el = unit.end_line,
            nodes = unit.body_node_count,
            hash = &unit.normalized_body_hash[..12],
        )
        .unwrap();
    }
    out
}

#[test]
fn extraction_matches_golden_snapshots() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extract");
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut checked = 0;
    for entry in std::fs::read_dir(&fixtures).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "expected") {
            continue;
        }
        let expected_path = path.with_extension(format!(
            "{}.expected",
            path.extension().unwrap().to_str().unwrap()
        ));
        let actual = snapshot(&path);
        if update {
            std::fs::write(&expected_path, &actual).unwrap();
        }
        let expected = std::fs::read_to_string(&expected_path).unwrap_or_else(|_| {
            panic!(
                "missing snapshot {} (run with UPDATE_GOLDEN=1)",
                expected_path.display()
            )
        });
        assert_eq!(actual, expected, "snapshot mismatch for {}", path.display());
        checked += 1;
    }
    assert_eq!(checked, 12, "expected one fixture per bundled language");
}

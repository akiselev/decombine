//! The ignore-file workflow: one reviewed cluster hash per line. Cluster
//! hashes are stable over relative paths + body hashes, so an ignored
//! cluster reappears if its code or location changes.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;

/// Load ignored cluster hashes. Missing file = empty set. `#` comments and
/// blank lines are allowed.
pub fn load_ignored_hashes(path: &Path) -> Result<HashSet<String>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => return Err(error.into()),
    };
    Ok(text
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hashes_with_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ignore.txt");
        std::fs::write(&path, "# reviewed 2026-07\nabc123\n\ndef456 # keep\n").unwrap();
        let hashes = load_ignored_hashes(&path).unwrap();
        assert_eq!(hashes.len(), 2);
        assert!(hashes.contains("abc123"));
        assert!(hashes.contains("def456"));
    }

    #[test]
    fn missing_file_is_empty() {
        assert!(
            load_ignored_hashes(Path::new("/nonexistent/x.txt"))
                .unwrap()
                .is_empty()
        );
    }
}

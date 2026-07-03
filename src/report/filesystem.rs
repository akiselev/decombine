//! Safe report-directory maintenance: only files this tool generates are
//! ever deleted (index.md, cluster-*.md, concerns/, compare/).

use std::path::Path;

use anyhow::Result;

fn is_generated_name(name: &str) -> bool {
    name == "index.md" || (name.starts_with("cluster-") && name.ends_with(".md"))
}

/// Remove previously generated report files from `dir`, leaving anything
/// the user may have placed there untouched.
pub fn clean_report_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_file() && is_generated_name(&name) {
            std::fs::remove_file(&path)?;
        } else if path.is_dir() && (name == "concerns" || name == "compare") {
            for sub in std::fs::read_dir(&path)? {
                let sub = sub?.path();
                if sub.is_file() && sub.extension().is_some_and(|e| e == "md") {
                    std::fs::remove_file(&sub)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_generated_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let write = |rel: &str| {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x").unwrap();
        };
        write("index.md");
        write("cluster-01.md");
        write("concerns/auth.md");
        write("compare/index.md");
        write("notes.md"); // user file: kept
        write("keep/cluster-01.md"); // outside managed dirs: kept

        clean_report_dir(dir.path()).unwrap();

        assert!(!dir.path().join("index.md").exists());
        assert!(!dir.path().join("cluster-01.md").exists());
        assert!(!dir.path().join("concerns/auth.md").exists());
        assert!(!dir.path().join("compare/index.md").exists());
        assert!(dir.path().join("notes.md").exists());
        assert!(dir.path().join("keep/cluster-01.md").exists());
    }

    #[test]
    fn missing_dir_is_fine() {
        clean_report_dir(Path::new("/nonexistent/report")).unwrap();
    }
}

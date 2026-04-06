//! Glob-based path exclusion for directory traversal.
//!
//! Provides [`compile_excludes`] to build a compiled [`GlobSet`] from user-supplied
//! patterns, and [`is_excluded`] to test whether a given path matches any of them.
//! Patterns are matched against each component of the path relative to the scan root,
//! so `"node_modules"` excludes any `node_modules/` directory at any depth.

use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;

/// Compile exclude patterns into a `GlobSet` for efficient matching.
/// Supports both exact names (e.g. `.git`) and glob patterns (e.g. `*.log`, `build*`).
/// Returns `(GlobSet, Vec<String>)` where the second element contains any invalid patterns.
///
/// # Examples
///
/// ```
/// use uninorm_core::compile_excludes;
///
/// let patterns = vec![".git".to_string(), "*.log".to_string()];
/// let (globs, invalid) = compile_excludes(&patterns);
/// assert!(invalid.is_empty());
/// assert!(globs.is_match(".git"));
/// assert!(globs.is_match("app.log"));
/// assert!(!globs.is_match("readme.md"));
/// ```
pub fn compile_excludes(patterns: &[String]) -> (GlobSet, Vec<String>) {
    let mut builder = GlobSetBuilder::new();
    let mut invalid = Vec::new();
    for pat in patterns {
        match Glob::new(pat) {
            Ok(glob) => {
                builder.add(glob);
            }
            Err(_) => {
                invalid.push(pat.clone());
            }
        }
    }
    let set = builder.build().unwrap_or_else(|_| {
        GlobSetBuilder::new()
            .build()
            .expect("empty GlobSet must build")
    });
    (set, invalid)
}

/// Check if a path should be excluded based on compiled glob patterns.
/// Matches against each component of the relative path (from root).
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use uninorm_core::{compile_excludes, is_excluded};
///
/// let patterns = vec![".git".to_string(), "*.log".to_string()];
/// let (globs, _) = compile_excludes(&patterns);
///
/// assert!(is_excluded(Path::new("/root/.git/config"), Path::new("/root"), &globs));
/// assert!(!is_excluded(Path::new("/root/src/main.rs"), Path::new("/root"), &globs));
/// ```
pub fn is_excluded(entry_path: &Path, root: &Path, globs: &GlobSet) -> bool {
    if globs.is_empty() {
        return false;
    }
    let relative = entry_path.strip_prefix(root).unwrap_or(entry_path);
    relative.components().any(|c| {
        if let std::path::Component::Normal(name) = c {
            let s = name.to_string_lossy();
            globs.is_match(s.as_ref())
        } else {
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_excludes_with_invalid_pattern() {
        let patterns = vec!["[invalid".to_string(), ".git".to_string()];
        let (globs, invalid) = compile_excludes(&patterns);
        assert_eq!(invalid.len(), 1);
        assert_eq!(invalid[0], "[invalid");
        assert!(globs.is_match(".git"));
    }

    #[test]
    fn test_compile_excludes_empty() {
        let (globs, invalid) = compile_excludes(&[]);
        assert!(invalid.is_empty());
        assert!(globs.is_empty());
    }

    #[test]
    fn test_exclude_relative_path_strips_watch_root_prefix() {
        use std::path::PathBuf;

        let watch_root = PathBuf::from("/home/user/node_modules/myproject");
        let file_path = watch_root.join("cafe\u{0301}.txt");

        let exclude = ["node_modules".to_string()];
        let relative = [watch_root.as_path()]
            .iter()
            .find_map(|root| file_path.strip_prefix(root).ok())
            .unwrap_or(&file_path);

        let excluded = relative.components().any(|c| {
            if let std::path::Component::Normal(n) = c {
                let s = n.to_string_lossy();
                exclude.iter().any(|pat| s.as_ref() == pat.as_str())
            } else {
                false
            }
        });

        assert!(
            !excluded,
            "file directly under watch root must not be excluded even if root path contains 'node_modules'"
        );
    }

    #[test]
    fn test_exclude_relative_path_matches_subdir_component() {
        use std::path::PathBuf;

        let watch_root = PathBuf::from("/home/user/myproject");
        let file_path = watch_root
            .join("node_modules")
            .join("some_pkg")
            .join("cafe\u{0301}.txt");

        let exclude = ["node_modules".to_string()];
        let relative = [watch_root.as_path()]
            .iter()
            .find_map(|root| file_path.strip_prefix(root).ok())
            .unwrap_or(&file_path);

        let excluded = relative.components().any(|c| {
            if let std::path::Component::Normal(n) = c {
                let s = n.to_string_lossy();
                exclude.iter().any(|pat| s.as_ref() == pat.as_str())
            } else {
                false
            }
        });

        assert!(
            excluded,
            "path with 'node_modules' component under watch root must be excluded"
        );
    }
}

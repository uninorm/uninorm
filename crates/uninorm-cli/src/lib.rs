//! Helper utilities for the `uninorm` CLI.
//!
//! Provides parsing and formatting functions used by the CLI binary:
//! [`parse_size`] for human-readable byte sizes, [`format_size`] for display,
//! and [`parse_indices`] for comma-separated entry selection.

/// Parse human-readable size strings like "100MB", "1GB", "500KB", or raw bytes.
pub fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim().to_uppercase();
    let (num_str, multiplier) = if let Some(n) = s.strip_suffix("GB") {
        (n, 1024 * 1024 * 1024u64)
    } else if let Some(n) = s.strip_suffix("MB") {
        (n, 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("KB") {
        (n, 1024)
    } else if let Some(n) = s.strip_suffix('B') {
        (n, 1)
    } else {
        (s.as_str(), 1)
    };
    let num: f64 = num_str
        .trim()
        .parse()
        .map_err(|_| format!("Invalid size: {s}"))?;
    if !num.is_finite() || num <= 0.0 {
        return Err(format!("Invalid size: {s}"));
    }
    let result = num * multiplier as f64;
    if result > u64::MAX as f64 {
        return Err(format!("Size too large: {s}"));
    }
    Ok(result as u64)
}

/// Format a byte count into a human-readable string.
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1}GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{}MB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{bytes}B")
    }
}

/// Parse comma-separated 1-based indices (e.g. "1,3,5") and validate against entry count.
/// Returns sorted, deduplicated 0-based indices.
pub fn parse_indices(s: &str, count: usize) -> Result<Vec<usize>, String> {
    let mut indices = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let n: usize = part
            .parse()
            .map_err(|_| format!("Invalid number: {part}"))?;
        if n == 0 || n > count {
            return Err(format!(
                "Entry #{n} does not exist. Use `uninorm watch list` to see entries (1-{count})."
            ));
        }
        indices.push(n - 1);
    }
    indices.sort_unstable();
    indices.dedup();
    if indices.is_empty() {
        return Err("No entry numbers provided.".to_string());
    }
    Ok(indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_size ──────────────────────────────────────────────────────────

    #[test]
    fn test_parse_size_megabytes() {
        assert_eq!(parse_size("100MB").unwrap(), 100 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_gigabytes() {
        assert_eq!(parse_size("1GB").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_kilobytes() {
        assert_eq!(parse_size("512KB").unwrap(), 512 * 1024);
    }

    #[test]
    fn test_parse_size_bytes() {
        assert_eq!(parse_size("4096B").unwrap(), 4096);
    }

    #[test]
    fn test_parse_size_raw_number() {
        assert_eq!(parse_size("1024").unwrap(), 1024);
    }

    #[test]
    fn test_parse_size_decimal() {
        assert_eq!(
            parse_size("1.5GB").unwrap(),
            (1.5 * 1024.0 * 1024.0 * 1024.0) as u64
        );
    }

    #[test]
    fn test_parse_size_case_insensitive() {
        assert_eq!(parse_size("50mb").unwrap(), 50 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_with_whitespace() {
        assert_eq!(parse_size("  100MB  ").unwrap(), 100 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_invalid_string() {
        assert!(parse_size("abc").is_err());
    }

    #[test]
    fn test_parse_size_negative() {
        assert!(parse_size("-1MB").is_err());
    }

    #[test]
    fn test_parse_size_zero() {
        assert!(parse_size("0MB").is_err());
    }

    #[test]
    fn test_parse_size_infinity() {
        assert!(parse_size("infMB").is_err());
    }

    // ── format_size ─────────────────────────────────────────────────────────

    #[test]
    fn test_format_size_gigabytes() {
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.0GB");
    }

    #[test]
    fn test_format_size_megabytes() {
        assert_eq!(format_size(50 * 1024 * 1024), "50MB");
    }

    #[test]
    fn test_format_size_kilobytes() {
        assert_eq!(format_size(512 * 1024), "512KB");
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(42), "42B");
    }

    #[test]
    fn test_format_size_zero() {
        assert_eq!(format_size(0), "0B");
    }

    #[test]
    fn test_format_size_boundary_mb() {
        assert_eq!(format_size(1024 * 1024), "1MB");
    }

    #[test]
    fn test_format_size_boundary_kb() {
        assert_eq!(format_size(1024), "1KB");
    }

    // ── parse_indices ───────────────────────────────────────────────────────

    #[test]
    fn test_parse_indices_single() {
        assert_eq!(parse_indices("2", 5).unwrap(), vec![1]);
    }

    #[test]
    fn test_parse_indices_multiple() {
        assert_eq!(parse_indices("1,3,5", 5).unwrap(), vec![0, 2, 4]);
    }

    #[test]
    fn test_parse_indices_dedup() {
        assert_eq!(parse_indices("2,2,3", 5).unwrap(), vec![1, 2]);
    }

    #[test]
    fn test_parse_indices_sorted() {
        assert_eq!(parse_indices("5,1,3", 5).unwrap(), vec![0, 2, 4]);
    }

    #[test]
    fn test_parse_indices_with_spaces() {
        assert_eq!(parse_indices(" 1 , 3 ", 5).unwrap(), vec![0, 2]);
    }

    #[test]
    fn test_parse_indices_out_of_range() {
        assert!(parse_indices("6", 5).is_err());
    }

    #[test]
    fn test_parse_indices_zero() {
        assert!(parse_indices("0", 5).is_err());
    }

    #[test]
    fn test_parse_indices_invalid_number() {
        assert!(parse_indices("abc", 5).is_err());
    }

    #[test]
    fn test_parse_indices_empty() {
        assert!(parse_indices("", 5).is_err());
    }

    #[test]
    fn test_parse_indices_trailing_comma() {
        assert_eq!(parse_indices("1,2,", 5).unwrap(), vec![0, 1]);
    }
}

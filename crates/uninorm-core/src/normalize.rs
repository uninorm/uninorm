use unicode_normalization::UnicodeNormalization;

/// Convert text content from NFD to NFC.
/// Use this for file contents (standard Unicode NFD).
///
/// # Examples
///
/// ```
/// use uninorm_core::to_nfc;
///
/// // Latin é in NFD (e + combining acute) → NFC (precomposed é)
/// assert_eq!(to_nfc("cafe\u{0301}"), "café");
///
/// // Already NFC text passes through unchanged
/// assert_eq!(to_nfc("hello world"), "hello world");
///
/// // Korean Hangul jamo → precomposed syllable
/// assert_eq!(to_nfc("\u{1100}\u{1161}\u{11BC}"), "강");
/// ```
pub fn to_nfc(s: &str) -> String {
    s.nfc().collect()
}

/// Convert a filename from NFD to NFC.
///
/// On macOS: uses `hfs_nfd` to handle the HFS+/APFS non-standard NFD variant,
/// which correctly composes Korean Hangul jamo and other HFS+ quirks.
///
/// On Linux/Windows: uses standard Unicode NFC normalization, which is correct
/// for those filesystems (ext4, NTFS, etc. store filenames as-is).
///
/// # Examples
///
/// ```
/// use uninorm_core::to_nfc_filename;
///
/// // Latin diacritics
/// assert_eq!(to_nfc_filename("re\u{0301}sume\u{0301}.pdf"), "résumé.pdf");
///
/// // ASCII filenames pass through unchanged
/// assert_eq!(to_nfc_filename("readme.md"), "readme.md");
/// ```
pub fn to_nfc_filename(s: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        hfs_nfd::compose_from_hfs_nfd(s)
    }

    #[cfg(not(target_os = "macos"))]
    {
        s.nfc().collect()
    }
}

/// Returns true if the string is already in NFC form.
///
/// # Examples
///
/// ```
/// use uninorm_core::is_nfc;
///
/// assert!(is_nfc("café"));       // precomposed NFC
/// assert!(is_nfc("hello"));      // pure ASCII
/// assert!(!is_nfc("e\u{0301}")); // NFD decomposed é
/// ```
pub fn is_nfc(s: &str) -> bool {
    unicode_normalization::is_nfc(s)
}

/// Returns true if the filename needs NFD → NFC conversion.
///
/// # Examples
///
/// ```
/// use uninorm_core::needs_filename_conversion;
///
/// assert!(needs_filename_conversion("e\u{0301}"));  // NFD é
/// assert!(!needs_filename_conversion("café"));       // already NFC
/// assert!(!needs_filename_conversion("hello.txt"));  // pure ASCII
/// ```
pub fn needs_filename_conversion(s: &str) -> bool {
    to_nfc_filename(s) != s
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Korean Hangul ─────────────────────────────────────────────────────────
    // HFS+ NFD decomposes Hangul syllables into individual jamo (lead + vowel +
    // optional trail), which differs from standard Unicode NFD.

    #[test]
    fn test_korean_syllable() {
        // 강 (U+AC15) → ᄀ (U+1100) + ᅡ (U+1161) + ᆼ (U+11BC)
        let nfd = "\u{1100}\u{1161}\u{11BC}";
        assert_eq!(to_nfc_filename(nfd), "강");
    }

    #[test]
    fn test_korean_word() {
        // 강남구 in HFS+ NFD jamo decomposition
        let nfd = "\u{1100}\u{1161}\u{11BC}\u{1102}\u{1161}\u{11B7}\u{1100}\u{116E}";
        assert_eq!(to_nfc_filename(nfd), "강남구");
    }

    #[test]
    fn test_korean_open_syllable() {
        // 가 (U+AC00) → ᄀ (U+1100) + ᅡ (U+1161), no trail jamo
        let nfd = "\u{1100}\u{1161}";
        assert_eq!(to_nfc_filename(nfd), "가");
    }

    #[test]
    fn test_korean_needs_conversion() {
        let nfd = "\u{1100}\u{1161}\u{11BC}"; // 강 in HFS+ NFD
        assert!(needs_filename_conversion(nfd));
        assert!(!needs_filename_conversion("강남구"));
    }

    // ── Latin with combining diacritics ───────────────────────────────────────
    // Standard Unicode NFD. HFS+ NFD behaves identically for Latin characters.

    #[test]
    fn test_latin_acute() {
        // é: e (U+0065) + combining acute (U+0301) → é (U+00E9)
        let nfd = "e\u{0301}";
        assert_eq!(to_nfc(nfd), "\u{00E9}");
        assert_eq!(to_nfc_filename(nfd), "\u{00E9}");
    }

    #[test]
    fn test_latin_tilde() {
        // ñ: n (U+006E) + combining tilde (U+0303) → ñ (U+00F1)
        let nfd = "n\u{0303}";
        assert_eq!(to_nfc(nfd), "\u{00F1}");
    }

    #[test]
    fn test_latin_diaeresis() {
        // ü: u (U+0075) + combining diaeresis (U+0308) → ü (U+00FC)
        let nfd = "u\u{0308}";
        assert_eq!(to_nfc(nfd), "\u{00FC}");
    }

    #[test]
    fn test_latin_word() {
        // "café" with é in NFD
        let nfd = "cafe\u{0301}";
        assert_eq!(to_nfc(nfd), "café");
        assert_eq!(to_nfc_filename(nfd), "café");
    }

    // ── Japanese Kana with combining marks ────────────────────────────────────
    // macOS decomposes voiced (dakuten ゛) and semi-voiced (handakuten ゜) kana
    // into base kana + combining mark, same as standard Unicode NFD.
    //
    // Combining dakuten:     U+3099 (voiced)
    // Combining handakuten:  U+309A (semi-voiced)

    #[test]
    fn test_japanese_hiragana_dakuten() {
        // が (U+304C) → か (U+304B) + ゛ (U+3099)
        let nfd = "\u{304B}\u{3099}";
        assert_eq!(to_nfc(nfd), "が");
        assert_eq!(to_nfc_filename(nfd), "が");
    }

    #[test]
    fn test_japanese_katakana_dakuten() {
        // ガ (U+30AC) → カ (U+30AB) + ゛ (U+3099)
        let nfd = "\u{30AB}\u{3099}";
        assert_eq!(to_nfc(nfd), "ガ");
        assert_eq!(to_nfc_filename(nfd), "ガ");
    }

    #[test]
    fn test_japanese_hiragana_handakuten() {
        // ぱ (U+3071) → は (U+306F) + ゜ (U+309A)
        let nfd = "\u{306F}\u{309A}";
        assert_eq!(to_nfc(nfd), "ぱ");
        assert_eq!(to_nfc_filename(nfd), "ぱ");
    }

    #[test]
    fn test_japanese_katakana_handakuten() {
        // パ (U+30D1) → ハ (U+30CF) + ゜ (U+309A)
        let nfd = "\u{30CF}\u{309A}";
        assert_eq!(to_nfc(nfd), "パ");
        assert_eq!(to_nfc_filename(nfd), "パ");
    }

    #[test]
    fn test_japanese_word() {
        // "ガジェット" with ガ and ジ in NFD
        // ガ = カ + U+3099,  ジ (U+30B8) = シ (U+30B7) + U+3099
        let nfd = "\u{30AB}\u{3099}\u{30B7}\u{3099}\u{30A7}\u{30C3}\u{30C8}";
        assert_eq!(to_nfc(nfd), "ガジェット");
    }

    #[test]
    fn test_japanese_needs_conversion() {
        let nfd_ga = "\u{304B}\u{3099}"; // が in NFD
        assert!(needs_filename_conversion(nfd_ga));
        assert!(!needs_filename_conversion("が"));
    }

    // ── Common cases ──────────────────────────────────────────────────────────

    #[test]
    fn test_already_nfc_passthrough() {
        assert_eq!(to_nfc("강남구 hello が"), "강남구 hello が");
        assert!(is_nfc("강남구 hello が"));
    }

    #[test]
    fn test_ascii_unchanged() {
        let s = "hello world 123";
        assert_eq!(to_nfc(s), s);
        assert_eq!(to_nfc_filename(s), s);
        assert!(!needs_filename_conversion(s));
    }

    // ── Empty / whitespace ────────────────────────────────────────────────────

    #[test]
    fn test_empty_string() {
        assert_eq!(to_nfc(""), "");
        assert_eq!(to_nfc_filename(""), "");
        assert!(is_nfc(""));
        assert!(!needs_filename_conversion(""));
    }

    #[test]
    fn test_whitespace_unchanged() {
        let s = "  \t\n ";
        assert_eq!(to_nfc(s), s);
        assert!(is_nfc(s));
    }

    // ── is_nfc correctness ────────────────────────────────────────────────────

    #[test]
    fn test_is_nfc_false_for_latin_nfd() {
        assert!(!is_nfc("e\u{0301}")); // é in NFD
        assert!(!is_nfc("n\u{0303}")); // ñ in NFD
        assert!(!is_nfc("u\u{0308}")); // ü in NFD
    }

    #[test]
    fn test_is_nfc_false_for_korean_hfs_nfd() {
        let nfd = "\u{1100}\u{1161}\u{11BC}"; // 강 in HFS+ NFD
        assert!(!is_nfc(nfd));
    }

    #[test]
    fn test_is_nfc_false_for_japanese_nfd() {
        assert!(!is_nfc("\u{304B}\u{3099}")); // が decomposed
    }

    #[test]
    fn test_is_nfc_true_for_nfc_strings() {
        assert!(is_nfc("hello"));
        assert!(is_nfc("café")); // NFC é (U+00E9)
        assert!(is_nfc("강남구"));
        assert!(is_nfc("が"));
    }

    // ── needs_filename_conversion: NFC input returns false ────────────────────

    #[test]
    fn test_needs_conversion_nfc_is_false() {
        assert!(!needs_filename_conversion("café"));
        assert!(!needs_filename_conversion("강남구"));
        assert!(!needs_filename_conversion("ガジェット"));
        assert!(!needs_filename_conversion("hello.txt"));
    }

    // ── Idempotency ───────────────────────────────────────────────────────────

    #[test]
    fn test_to_nfc_idempotent() {
        let nfd = "cafe\u{0301}";
        let once = to_nfc(nfd);
        assert_eq!(to_nfc(&once), once);
    }

    #[test]
    fn test_to_nfc_filename_idempotent() {
        let nfd = "\u{1100}\u{1161}\u{11BC}"; // 강 in HFS+ NFD
        let once = to_nfc_filename(nfd);
        assert_eq!(to_nfc_filename(&once), once);
    }

    // ── Mixed scripts in one string ───────────────────────────────────────────

    #[test]
    fn test_mixed_scripts_filename() {
        // Korean + Latin + Japanese, all in HFS+ NFD combining form
        let nfd = "\u{1100}\u{1161}\u{11BC} cafe\u{0301} \u{30AB}\u{3099}";
        let nfc = to_nfc_filename(nfd);
        assert_eq!(nfc, "강 café ガ");
        assert!(is_nfc(&nfc));
    }

    // ── CJK Unified Ideographs (already NFC, no conversion) ──────────────────

    #[test]
    fn test_cjk_unchanged() {
        let cjk = "日本語中文한자";
        assert_eq!(to_nfc(cjk), cjk);
        assert_eq!(to_nfc_filename(cjk), cjk);
        assert!(is_nfc(cjk));
        assert!(!needs_filename_conversion(cjk));
    }

    // ── Emoji (pre-composed, already NFC) ─────────────────────────────────────

    #[test]
    fn test_emoji_unchanged() {
        let emoji = "🦀🎉✅";
        assert_eq!(to_nfc(emoji), emoji);
        assert_eq!(to_nfc_filename(emoji), emoji);
        assert!(is_nfc(emoji));
    }

    // ── Filename with path separators and extensions ──────────────────────────

    #[test]
    fn test_filename_with_extension_and_nfd() {
        // "résumé.pdf" with both é in NFD
        let nfd = "re\u{0301}sume\u{0301}.pdf";
        assert_eq!(to_nfc(nfd), "résumé.pdf");
        assert_eq!(to_nfc_filename(nfd), "résumé.pdf");
    }

    // ── Unicode numbers / symbols unchanged ───────────────────────────────────

    #[test]
    fn test_numbers_and_symbols_unchanged() {
        let s = "123 !@#$%^&*()_+-=[]{}|;':\",./<>?";
        assert_eq!(to_nfc(s), s);
        assert!(!needs_filename_conversion(s));
    }

    // ── Singleton decomposition ───────────────────────────────────────────────

    #[test]
    fn test_singleton_decomposition() {
        // U+2126 OHM SIGN normalizes to U+03A9 GREEK CAPITAL LETTER OMEGA
        assert_eq!(to_nfc("\u{2126}"), "\u{03A9}");
        assert!(!is_nfc("\u{2126}"));
    }

    // ── Multiple combining marks ──────────────────────────────────────────────

    #[test]
    fn test_multiple_combining_marks() {
        // ệ = e + combining circumflex + combining dot below
        let nfd = "e\u{0302}\u{0323}";
        let nfc = to_nfc(nfd);
        assert!(is_nfc(&nfc));
        assert_ne!(nfc, nfd);
    }
}

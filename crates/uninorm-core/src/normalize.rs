use unicode_normalization::UnicodeNormalization;

/// Convert text content from NFD to NFC.
/// Use this for file contents (standard Unicode NFD).
pub fn to_nfc(s: &str) -> String {
    s.nfc().collect()
}

/// Convert a filename from HFS+ NFD to NFC.
/// macOS HFS+/APFS uses a non-standard NFD variant for filenames.
/// This correctly handles Korean Hangul jamo and other HFS+ quirks.
pub fn to_nfc_filename(s: &str) -> String {
    hfs_nfd::compose_from_hfs_nfd(s)
}

/// Returns true if the string is already in NFC form.
pub fn is_nfc(s: &str) -> bool {
    unicode_normalization::is_nfc(s)
}

/// Returns true if the filename needs HFS+ NFD → NFC conversion.
pub fn needs_filename_conversion(s: &str) -> bool {
    hfs_nfd::compose_from_hfs_nfd(s) != s
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
}

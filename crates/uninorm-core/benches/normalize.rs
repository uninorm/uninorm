use criterion::{black_box, criterion_group, criterion_main, Criterion};
use uninorm_core::{compile_excludes, convert_text, is_nfc, to_nfc, to_nfc_filename};

// ── Test data ───────────────────────────────────────────────────────────────

/// Korean Hangul in NFD (jamo decomposition): 강남구
const KOREAN_NFD: &str = "\u{1100}\u{1161}\u{11BC}\u{1102}\u{1161}\u{11B7}\u{1100}\u{116E}";
/// Latin with combining diacritics: café résumé
const LATIN_NFD: &str = "cafe\u{0301} re\u{0301}sume\u{0301}";
/// Japanese kana with dakuten: ガジェット
const JAPANESE_NFD: &str = "\u{30AB}\u{3099}\u{30B7}\u{3099}\u{30A7}\u{30C3}\u{30C8}";
/// Mixed scripts in NFD
const MIXED_NFD: &str = "\u{1100}\u{1161}\u{11BC} cafe\u{0301} \u{30AB}\u{3099}";
/// Already-NFC Korean text
const KOREAN_NFC: &str = "강남구 서울특별시 대한민국";
/// Long ASCII (no conversion needed)
const ASCII_LONG: &str =
    "The quick brown fox jumps over the lazy dog. 0123456789 abcdefghijklmnopqrstuvwxyz";

// ── to_nfc benchmarks ───────────────────────────────────────────────────────

fn bench_to_nfc(c: &mut Criterion) {
    let mut group = c.benchmark_group("to_nfc");

    group.bench_function("korean_nfd", |b| b.iter(|| to_nfc(black_box(KOREAN_NFD))));
    group.bench_function("latin_nfd", |b| b.iter(|| to_nfc(black_box(LATIN_NFD))));
    group.bench_function("japanese_nfd", |b| {
        b.iter(|| to_nfc(black_box(JAPANESE_NFD)))
    });
    group.bench_function("mixed_nfd", |b| b.iter(|| to_nfc(black_box(MIXED_NFD))));
    group.bench_function("already_nfc", |b| b.iter(|| to_nfc(black_box(KOREAN_NFC))));
    group.bench_function("ascii_long", |b| b.iter(|| to_nfc(black_box(ASCII_LONG))));

    group.finish();
}

// ── to_nfc_filename benchmarks ──────────────────────────────────────────────

fn bench_to_nfc_filename(c: &mut Criterion) {
    let mut group = c.benchmark_group("to_nfc_filename");

    group.bench_function("korean_nfd", |b| {
        b.iter(|| to_nfc_filename(black_box(KOREAN_NFD)))
    });
    group.bench_function("latin_nfd", |b| {
        b.iter(|| to_nfc_filename(black_box(LATIN_NFD)))
    });
    group.bench_function("already_nfc", |b| {
        b.iter(|| to_nfc_filename(black_box("강남구.txt")))
    });

    group.finish();
}

// ── is_nfc benchmarks ───────────────────────────────────────────────────────

fn bench_is_nfc(c: &mut Criterion) {
    let mut group = c.benchmark_group("is_nfc");

    group.bench_function("nfc_true_korean", |b| {
        b.iter(|| is_nfc(black_box(KOREAN_NFC)))
    });
    group.bench_function("nfc_false_nfd", |b| {
        b.iter(|| is_nfc(black_box(KOREAN_NFD)))
    });
    group.bench_function("nfc_true_ascii", |b| {
        b.iter(|| is_nfc(black_box(ASCII_LONG)))
    });

    group.finish();
}

// ── convert_text benchmarks ─────────────────────────────────────────────────

fn bench_convert_text(c: &mut Criterion) {
    // Build a larger document with repeated NFD content
    let large_nfd: String = std::iter::repeat_n(MIXED_NFD, 100)
        .collect::<Vec<_>>()
        .join("\n");
    let large_nfc: String = std::iter::repeat_n(KOREAN_NFC, 100)
        .collect::<Vec<_>>()
        .join("\n");

    let mut group = c.benchmark_group("convert_text");

    group.bench_function("large_nfd_doc", |b| {
        b.iter(|| convert_text(black_box(&large_nfd)))
    });
    group.bench_function("large_nfc_doc", |b| {
        b.iter(|| convert_text(black_box(&large_nfc)))
    });

    group.finish();
}

// ── compile_excludes benchmarks ─────────────────────────────────────────────

fn bench_compile_excludes(c: &mut Criterion) {
    let patterns: Vec<String> = vec![
        ".git".into(),
        "node_modules".into(),
        "*.pyc".into(),
        "*.log".into(),
        "__pycache__".into(),
        ".DS_Store".into(),
        "target".into(),
        "build*".into(),
    ];

    c.bench_function("compile_excludes_8_patterns", |b| {
        b.iter(|| compile_excludes(black_box(&patterns)))
    });
}

criterion_group!(
    benches,
    bench_to_nfc,
    bench_to_nfc_filename,
    bench_is_nfc,
    bench_convert_text,
    bench_compile_excludes,
);
criterion_main!(benches);

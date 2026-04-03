export PATH := $(HOME)/.cargo/bin:$(PATH)

.PHONY: build test install clean fmt clippy check bench

# ── Development ───────────────────────────────────────────────────────────────

build:
	cargo build --workspace

test:
	cargo test --workspace

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

## Run fmt, clippy, and test (pre-push checklist)
check: fmt
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

bench:
	cargo bench --package uninorm-core

# ── CLI ───────────────────────────────────────────────────────────────────────

install:
	cargo install --path crates/uninorm-cli

# ── Misc ──────────────────────────────────────────────────────────────────────

clean:
	cargo clean

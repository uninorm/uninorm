.PHONY: build test bundle universal install release-gui icons clean

# ── Development ───────────────────────────────────────────────────────────────

build:
	cargo build --workspace

test:
	cargo test --workspace

# ── CLI ───────────────────────────────────────────────────────────────────────

install:
	cargo install --path crates/uninorm-cli

# ── GUI ───────────────────────────────────────────────────────────────────────

## Generate app.icns and menubar PNGs from SVG sources (requires: brew install librsvg)
icons:
	crates/uninorm-gui/assets/icons/generate.sh

## Build and launch the GUI in debug mode
run-gui:
	cargo run --package uninorm-gui

## Build the macOS .app bundle (requires cargo-bundle)
bundle:
	cargo bundle --release --package uninorm-gui
	@echo "App bundle: target/release/bundle/osx/uninorm.app"

## Build a universal binary (arm64 + x86_64) and create a .app bundle
universal:
	cargo build --release --target aarch64-apple-darwin --package uninorm-gui
	cargo build --release --target x86_64-apple-darwin  --package uninorm-gui
	lipo -create \
		target/aarch64-apple-darwin/release/uninorm-gui \
		target/x86_64-apple-darwin/release/uninorm-gui \
		-output target/release/uninorm-gui
	cargo bundle --release --package uninorm-gui
	@echo "Universal .app bundle: target/release/bundle/osx/uninorm.app"

## Zip the .app bundle for distribution
release-gui: bundle
	cd target/release/bundle/osx && zip -r ../../../../uninorm-gui.app.zip uninorm.app
	@echo "Release archive: uninorm-gui.app.zip"

# ── Misc ──────────────────────────────────────────────────────────────────────

clean:
	cargo clean

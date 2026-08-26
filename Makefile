.PHONY: app bundle test check

# Run the desktop app in dev mode (vite + tauri, live reload).
app:
	cargo tauri dev

# Build the distributable bundle (target/release/bundle/).
bundle:
	cargo tauri build

# Core library tests.
test:
	cargo test -p oikonomia-core

# Full quality gate, mirroring CI.
check:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo deny check
	cargo test -p oikonomia-core
	cd web && npx tsc -b && npm test && npm run build

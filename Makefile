.PHONY: app bundle test check smoke

# Run the desktop app in dev mode (vite + tauri, live reload).
app:
	cargo tauri dev

# Build the distributable bundle (target/release/bundle/).
bundle:
	cargo tauri build -- --locked

# Build the .app bundle and verify it renders (never smoke the bare binary).
smoke:
	./scripts/smoke-macos.sh

# Core library tests.
test:
	cargo test -p oikonomia-core --locked

# Local quality gate (a subset of CI).
check:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
	cargo deny check
	./scripts/assert-core-offline.sh
	cargo test -p oikonomia-core --locked
	cd web && npx tsc -b && npm test && npm run build

.PHONY: app bundle test check doc smoke

# The Tauri CLI pinned in web/package-lock.json, the copy the CI workflows
# run. A globally installed `cargo tauri` may be another version.
TAURI_CLI := web/node_modules/@tauri-apps/cli/tauri.js

$(TAURI_CLI): web/package-lock.json
	cd web && npm ci

# Run the desktop app in dev mode (vite + tauri, live reload).
app: $(TAURI_CLI)
	node $(TAURI_CLI) dev -- --locked

# Build the distributable bundle (target/release/bundle/).
bundle: $(TAURI_CLI)
	node $(TAURI_CLI) build -- --locked

# Build the .app bundle and verify it renders (never smoke the bare binary).
smoke: $(TAURI_CLI)
	./scripts/smoke-macos.sh

# Core library tests.
test:
	cargo test -p oikonomia-core --locked

# API docs with rustdoc warnings as errors, as the CI `docs` job runs them.
doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items --locked

# Local quality gate (a subset of CI).
check:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
	cargo deny check
	./scripts/assert-core-offline.sh
	cargo test -p oikonomia-core --locked
	cd web && npx tsc -b && npm test && npm run build

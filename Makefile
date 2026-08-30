# Single source of truth for build, test, and format operations.
# CI (.github/workflows/ci.yml) invokes these exact targets, so a change
# made here applies identically locally and in CI. Keep the recipes in sync
# with the README's Commands section. Each recipe is a single command line,
# so a failing command makes make abort and the failure propagates to CI.
.PHONY: build test clippy fmt fmt-check wasm clean

# Build the contract for native testing.
build:
	cargo build

# Run the unit test suite.
test:
	cargo test

# Lint production contract code with warnings promoted to errors. Test-target
# linting remains a separate migration because the repository's checked-in
# tests currently target an older Soroban test API.
clippy:
	cargo clippy --lib -- -D warnings

# Format the code in place.
fmt:
	cargo fmt --all

# Verify formatting (used in CI).
fmt-check:
	cargo fmt --all -- --check

# Build the optimized wasm artifact for deployment.
# Requires the wasm32-unknown-unknown target: rustup target add wasm32-unknown-unknown
wasm:
	cargo build --target wasm32-unknown-unknown --release

# Remove build artifacts.
clean:
	cargo clean

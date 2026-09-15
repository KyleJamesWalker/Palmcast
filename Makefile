.PHONY: test test-rust test-js test-e2e run lint fmt build

test: test-rust test-js

test-rust:
	cargo test

test-js:
	node --test web/*.test.mjs

# Not part of `test`: it builds the release binary and drives a real browser.
test-e2e:
	cd e2e && npm ci && npx playwright install --with-deps chromium && npx playwright test

run:
	cargo run -- --port 8080

lint:
	cargo clippy --all-targets -- -D warnings
	cargo fmt --check

fmt:
	cargo fmt

build:
	cargo build --release

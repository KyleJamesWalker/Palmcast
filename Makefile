.PHONY: test test-rust test-js run lint fmt build

test: test-rust test-js

test-rust:
	cargo test

test-js:
	node --test web/*.test.mjs

run:
	cargo run -- --port 8080

lint:
	cargo clippy --all-targets -- -D warnings
	cargo fmt --check

fmt:
	cargo fmt

build:
	cargo build --release

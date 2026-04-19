.PHONY: build test clean check fmt lint doc run help

BINARY = nano-rs
CONFIG = config.json

help:
	@echo "NANO build commands:"
	@echo "  make build    - Build release binary"
	@echo "  make test     - Run all tests"
	@echo "  make check    - Fast check (no build)"
	@echo "  make fmt      - Format code"
	@echo "  make lint     - Run clippy"
	@echo "  make clean    - Clean build artifacts"
	@echo "  make doc      - Build documentation"
	@echo "  make run      - Build and run with config.json"

build:
	cargo build --release
	@echo "Binary: target/release/$(BINARY)"

test:
	cargo test --all

check:
	cargo check

fmt:
	cargo fmt

lint:
	cargo clippy --all-targets --all-features -- -D warnings

clean:
	cargo clean
	rm -rf target/

doc:
	cargo doc --no-deps --open

run: build
	./target/release/$(BINARY) --config $(CONFIG)

# Development build (faster)
dev:
	cargo build

# Run with logging
debug: dev
	RUST_LOG=debug ./target/debug/$(BINARY) --config $(CONFIG)

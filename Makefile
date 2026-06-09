.PHONY: build check fmt install-local package run

build:
	cargo build --release --locked

check:
	cargo fmt --check
	cargo clippy --locked --all-targets --all-features -- -D warnings
	cargo test --locked
	cargo build --release --locked
	cargo package --allow-dirty --locked

fmt:
	cargo fmt

install-local:
	cargo build --release --locked
	mkdir -p "$$HOME/.local/bin"
	install -m 0755 target/release/profilr "$$HOME/.local/bin/profilr"

package:
	cargo package --locked

run:
	cargo run --locked -- run --limit 20

.PHONY: build run test fmt clean

all: build

build:
	cargo build --release

run:
	cargo run

test:
	cargo test

fmt:
	cargo fmt

clean:
	cargo clean

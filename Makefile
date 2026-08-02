.PHONY: build run test fmt clean package-linux package-mac package-windows

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

package-linux: build
	@echo "Packaging flow for Linux..."
	@rm -rf flow-linux-x86_64 flow-linux-x86_64.tar.gz
	@mkdir -p flow-linux-x86_64
	@cp target/release/flow flow-linux-x86_64/
	@cp -r resources flow-linux-x86_64/ 2>/dev/null || true
	@cp installer/setup_linux.sh flow-linux-x86_64/
	@chmod +x flow-linux-x86_64/setup_linux.sh
	@cp installer/flow.desktop flow-linux-x86_64/
	@cp README.md flow-linux-x86_64/
	@tar -czf flow-linux-x86_64.tar.gz flow-linux-x86_64
	@rm -rf flow-linux-x86_64
	@echo "Created distribution archive: flow-linux-x86_64.tar.gz"

package-mac: build
	@echo "Packaging flow for macOS (.dmg)..."
	@rm -rf dmg_root flow-macos.dmg
	@mkdir -p dmg_root/flow
	@cp target/release/flow dmg_root/flow/flow
	@cp -r resources dmg_root/flow/resources 2>/dev/null || true
	@hdiutil create -volname "flow" -srcfolder dmg_root/flow -ov -format UDZO flow-macos.dmg
	@rm -rf dmg_root
	@echo "Created macOS disk image: flow-macos.dmg"

package-windows: build
	@echo "Packaging flow for Windows (.setup.exe)..."
	@iscc installer/flow.iss

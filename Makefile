PREFIX ?= /usr/local
BINDIR := $(PREFIX)/bin

.PHONY: build install clean test

build:
	cargo build --release

install: build
	install -d $(BINDIR)
	install -m 755 target/release/sm $(BINDIR)/sm

clean:
	cargo clean

test:
	cargo test

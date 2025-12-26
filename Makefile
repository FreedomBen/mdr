#
# mdr: Rust-first CLI with optional static site helpers.
#
# Quick start:
#   make            # build debug binary (target/debug/mdr)
#   make test       # run unit/integration tests
#   make dist       # build release binary into dist/mdr
#   make site       # rebuild docs/ from site/src markdown
#   make watch      # serve docs/ and rebuild on changes
#

CARGO ?= cargo
CARGO_TARGET_DIR ?= $(CURDIR)/target
BIN := mdr
TARGET ?=

BIN_DEBUG := $(CARGO_TARGET_DIR)/debug/$(BIN)

ifeq ($(TARGET),)
TARGET_FLAG :=
BIN_RELEASE := $(CARGO_TARGET_DIR)/release/$(BIN)
else
TARGET_FLAG := --target $(TARGET)
BIN_RELEASE := $(CARGO_TARGET_DIR)/$(TARGET)/release/$(BIN)
endif

.PHONY: all
all: build

.PHONY: build
build: $(BIN_DEBUG)

$(BIN_DEBUG): Cargo.toml src/main.rs assets/template.html5 assets/css/theme.css assets/css/skylighting-solarized-theme.css assets/pandoc-sidenote.lua
	$(CARGO) build

.PHONY: dist
dist: dist/$(BIN)

dist/$(BIN): Cargo.toml src/main.rs assets/template.html5 assets/css/theme.css assets/css/skylighting-solarized-theme.css assets/pandoc-sidenote.lua
	CARGO_TARGET_DIR=$(CARGO_TARGET_DIR) $(CARGO) build --release $(TARGET_FLAG)
	mkdir -p dist
	cp $(BIN_RELEASE) dist/$(BIN)

.PHONY: fmt
fmt:
	$(CARGO) fmt

.PHONY: lint
lint:
	$(CARGO) clippy -- -D warnings

.PHONY: test
test:
	$(CARGO) test

.PHONY: watch-cli
watch-cli:
	cargo watch -x check -x test

.PHONY: clean
clean:
	rm -rf dist

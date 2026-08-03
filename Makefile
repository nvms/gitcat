REPOS ?= $(HOME)/code
BIND ?= 127.0.0.1:9090
SITE_NAME ?= gitcat

.PHONY: server
server: ## Run gitcat (release build). Override REPOS, BIND, SITE_NAME
	cargo run --release -- --repos $(REPOS) --bind $(BIND) --site-name "$(SITE_NAME)"

.PHONY: dev
dev: ## Run gitcat with a debug build, for faster recompiles while working on it
	cargo run -- --repos $(REPOS) --bind $(BIND) --site-name "$(SITE_NAME)"

.PHONY: build
build: ## Build the release binary into target/release/gitcat
	cargo build --release

.PHONY: install
install: ## Install gitcat onto PATH via cargo
	cargo install --path .

.PHONY: test
test: ## Run the test suite
	cargo test

.PHONY: fmt
fmt: ## Format the source
	cargo fmt

.PHONY: lint
lint: ## Run clippy with warnings denied
	cargo clippy --all-targets -- -D warnings

.PHONY: check
check: ## Run everything CI runs: format check, lint, tests
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo test

.PHONY: audit
audit: ## Report repository health
	./audit

.PHONY: clean
clean: ## Remove build artifacts
	cargo clean

.PHONY: help
help: ## Show help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(firstword $(MAKEFILE_LIST)) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[32m%-20s\033[0m %s\n", $$1, $$2}'

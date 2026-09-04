# graphql-http-rust — convenience targets
# All real CI gates live in .github/workflows/ci.yml; these are the local
# equivalents so `make ci` reproduces exactly what GitHub Actions enforces.

.PHONY: help build test clippy fmt fmt-check audit ci run-server k6 k6-smoke clean

help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

build: ## Build the whole workspace (debug)
	cargo build --workspace

test: ## Run all workspace tests (73 spec-mapped tests)
	cargo test --workspace

clippy: ## Lint with clippy, warnings are errors (same as CI)
	cargo clippy --workspace --all-targets -- -D warnings

fmt: ## Auto-format all code
	cargo fmt --all

fmt-check: ## Verify formatting without changing files (same as CI)
	cargo fmt --all --check

audit: ## Check dependencies for known advisories (non-blocking in CI)
	cargo audit

ci: fmt-check clippy test ## Run the full CI gate locally: fmt, clippy, tests

run-server: ## Start the example server on http://127.0.0.1:8080
	cargo run -p example-server

k6: ## Run the full k6 suite (smoke, query_load, mixed_ramp, error_path_spike)
	k6 run examples/k6/graphql-scenarios.js

k6-smoke: ## Run only the k6 spec-conformance smoke pass
	k6 run --env SCENARIO=smoke examples/k6/graphql-scenarios.js

example: ## Run the library usage example
	cargo run -p graphql-http-rust --example usage

clean: ## Remove build artifacts
	cargo clean

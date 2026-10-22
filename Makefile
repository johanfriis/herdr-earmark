.DEFAULT_GOAL := help

.PHONY: help
help: ## Show targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "}; {printf "  %-10s %s\n", $$1, $$2}'

.PHONY: check
check: ## fmt, clippy, tests, locked release build (what CI runs)
	cargo fmt --check
	cargo clippy --all-targets --locked -- -D warnings
	cargo test --locked
	cargo build --release --locked

.PHONY: release
release: ## Bump version, commit, tag vVERSION, push (make release VERSION=x.y.z)
	@echo "$(VERSION)" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$$' || { echo "usage: make release VERSION=x.y.z"; exit 1; }
	@[ "$$(git branch --show-current)" = main ] || { echo "release from main"; exit 1; }
	@[ -z "$$(git status --porcelain)" ] || { echo "working tree is not clean"; exit 1; }
	@! git rev-parse -q --verify "refs/tags/v$(VERSION)" >/dev/null || { echo "tag v$(VERSION) exists"; exit 1; }
	git pull --ff-only
	sed -i.bak 's/^version = ".*"/version = "$(VERSION)"/' Cargo.toml herdr-plugin.toml && rm Cargo.toml.bak herdr-plugin.toml.bak
	cargo update --workspace --offline
	$(MAKE) check
	git diff --quiet || git commit -am "chore: release v$(VERSION)"
	git tag -a "v$(VERSION)" -m "v$(VERSION)"
	git push --atomic origin main "v$(VERSION)"

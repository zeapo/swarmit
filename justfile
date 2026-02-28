# Swarmit — justfile

version := `grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/'`

# List available recipes
default:
    @just --list

# Build all targets
build:
    cargo build

# Run all tests
test:
    cargo test

# Run clippy lints
lint:
    cargo clippy --all-targets -- -D warnings

# Format code
fmt:
    cargo fmt --all

# Check formatting without modifying
fmt-check:
    cargo fmt --all -- --check

# Full CI check: fmt, lint, test
check: fmt-check lint test

# Dry-run publish (verify packaging without uploading)
publish-dry-run:
    cargo publish --dry-run

# Publish to crates.io
publish: check publish-dry-run
    @echo "Publishing swarmit v{{version}} to crates.io..."
    cargo publish
    @echo "Published!"

# Bump version (usage: just bump 1.2.0)
bump new_version:
    @echo "Bumping version to {{new_version}}..."
    sed -i '' 's/^version = ".*"/version = "{{new_version}}"/' Cargo.toml
    @echo "Updated workspace version to {{new_version}}"

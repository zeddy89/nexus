# Nexus development recipes

# Build in debug mode
dev:
    cargo build

# Build in release mode
release:
    cargo build --release

# Run all tests
test:
    cargo test

# Run clippy with warnings as errors
lint:
    cargo clippy -- -D warnings

# Format code
fmt:
    cargo fmt

# Run format, lint, and tests together
check: fmt lint test

# Install nexus locally
install:
    cargo install --path .

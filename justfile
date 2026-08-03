set shell := ["zsh", "-eu", "-o", "pipefail", "-c"]

cargo := env_var_or_default("CARGO", "cargo")
root := justfile_directory()

# cargo build minemu workspace
build:
  {{ cargo }} build --workspace

# cargo test minemu workspace
test:
  {{ cargo }} test --workspace

# cargo check minemu workspace
check:
  {{ cargo }} check --workspace --all-targets

# clipply lint workspace
lint:
  {{ cargo }} clippy --workspace --all-targets -- -D warnings

# rustfmt format workspace
fmt:
  {{ cargo }} fmt --all -- --check

# ci pipeline
ci: fmt check lint test

# cargo clean
clean:
  {{ cargo }} clean


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

# smoke-test CLI help
docs:
  {{ cargo }} run --quiet -p minemu -- --help >/dev/null
  {{ cargo }} run --quiet -p minemu -- image --help >/dev/null
  {{ cargo }} run --quiet -p minemu -- run --help >/dev/null
  {{ cargo }} run --quiet -p minemu -- test --help >/dev/null

# build the freestanding minimum template and reference examples
template:
  make -C "{{ root }}/minimum-template"
  make -C "{{ root }}/minimum-template" MINEMU="{{ cargo }} run --manifest-path {{ root }}/Cargo.toml -p minemu --" image

# build and run the minimum platform conformance tests
conformance:
  make -C "{{ root }}/minimum-tests" MINEMU="{{ cargo }} run --manifest-path {{ root }}/Cargo.toml -p minemu --" test

# ci pipeline
ci: fmt check lint test docs template conformance

# cargo clean
clean:
  {{ cargo }} clean

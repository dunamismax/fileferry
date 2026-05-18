set shell := ["sh", "-eu", "-c"]

fmt:
    cargo fmt --all --check

check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    cargo build --workspace

test:
    cargo test --workspace --all-features

test-s3:
    cargo test -p sealport-storage s3_store_round_trip_when_integration_env_is_enabled

build:
    cargo build --workspace

web:
    cargo run -p sealport-web

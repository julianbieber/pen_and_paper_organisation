# Building, checking and installing pnp and pnp-ctl.

default:
    @just --list

# Install pnp and pnp-ctl into ~/.cargo/bin; the tile strips are compiled in.
install:
    cargo install --locked --path crates/campaign_editor

# Run the editor, opening the campaign at `campaign` when one is given.
run *campaign:
    cargo run -p campaign_editor -- {{campaign}}

# Run every test, failing rather than skipping where zk or git is missing.
test:
    PNP_REQUIRE_ZK=1 PNP_REQUIRE_GIT=1 cargo test --workspace

# Clippy with warnings denied.
lint:
    cargo clippy --workspace -- -D warnings

# Everything a branch should pass before review.
check: lint test

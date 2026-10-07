#!/bin/sh
# every gate a pull request must pass, in the order ci runs them.
# run from anywhere: `tool/tasks/fullcheck.sh`
set -eu
cd "$(dirname "$0")/../.."

cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run --quiet -p anina -- doctor
cargo deny check
# the only warning expected here: the `leaks` test is not in the package.
cargo package --workspace --locked --allow-dirty

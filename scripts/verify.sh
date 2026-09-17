#!/usr/bin/env bash
set -euo pipefail

npm ci
npm test
npm run build
cargo fmt --all -- --check
cargo clippy -p sanctum-core --all-targets -- -D warnings
cargo test -p sanctum-core -- --test-threads=1

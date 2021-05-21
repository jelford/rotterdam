#!/usr/bin/env bash
set -euo pipefail
set LFS=$'\t\n'

export RUSTFLAGS="-D warnings"
cargo clippy

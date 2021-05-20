#!/usr/bin/env bash
set -euo pipefail
set IFS=$'\t\n'

cargo test --all --verbose

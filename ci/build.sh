#!/usr/bin/env bash
set -euo pipefail
set IFS=$'\t\n'

cargo build --examples --all --verbose

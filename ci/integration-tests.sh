#!/usr/bin/env bash
set -euo pipefail
set IFS=$'\n\t'


cargo build

export ROTTERDAM_BIN="$(pwd)/target/debug/rotterdam"
export RUST_LOG="debug,ureq=info"
export RUST_BACKTRACE=1

if [[ ! -x "${ROTTERDAM_BIN}" ]]; then
    echo >&2 "rotterdam debug executable not present at ${ROTTERDAM_BIN} after build; something's wrong!"
    exit 1
fi

cargo run -p test-rotterdam
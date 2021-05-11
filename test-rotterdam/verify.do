exec >&2
set -e

redo-always

redo-ifchange rotterdam-bin
: ${ROTTERDAM_BIN:=$(cat rotterdam-bin)}; export ROTTERDAM_BIN

cargo test -- --nocapture

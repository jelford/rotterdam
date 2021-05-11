
# Cargo implements incremental build
redo-always

cargo build >&2

cp -l target/debug/rotterdam $3
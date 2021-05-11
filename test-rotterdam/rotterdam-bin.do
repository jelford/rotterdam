exec >&2
set -euo pipefail

: ${ROTTERDAM_BIN:="$(git rev-parse --show-toplevel)/rotterdam-debug"}
redo-ifchange ${ROTTERDAM_BIN}
echo -n ${ROTTERDAM_BIN} > $3
sha256sum ${ROTTERDAM_BIN} | redo-stamp

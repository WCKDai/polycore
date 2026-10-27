#!/usr/bin/env bash
# The bench's `polykrom-cli`: Polykrom at a pinned revision plus bench/oracle/polykrom-bench.patch.
#
#   bench/oracle/build-cli.sh DIR [POLYKROM_REV]
#
# The patch changes nothing unless its environment variables are set. It adds:
# - the Nightfall backend (from Polykrom's unmerged feat/nightfall-backend branch): with
#   SOLIDCRAFT_NIGHTFALL=<nf> a feature's joins, cuts, intersects, constant fillets, equal
#   chamfers and inward shells run on OpenCASCADE through `nf kernel serve`;
# - POLYCORE_ORACLE_STRICT=1: a refused operation fails instead of falling back to truck;
# - POLYCORE_THREADS=N: caps the kernel's own thread pools (with RAYON_NUM_THREADS=N);
# - to each case's JSON: `oracle` (operations OpenCASCADE took and refused), `history` (faces
#   whose names follow exact kernel history) and `geometry_hash` (of every body's exact native
#   encoding).
set -euo pipefail

POLYKROM_REV=${2:-3422dd5fb58a5dcae7606bdee4686db186580a53}
JOBS=${JOBS:-4}
here=$(cd "$(dirname "$0")" && pwd)

dir=${1:?usage: build-cli.sh DIR [POLYKROM_REV]}
mkdir -p "$dir"
dir=$(cd "$dir" && pwd)
src="$dir/polykrom"

if [[ ! -d "$src/.git" ]]; then
  git init -q "$src"
  git -C "$src" remote add origin https://github.com/WCKDai/polykrom.git
fi
git -C "$src" fetch -q --depth 1 origin "$POLYKROM_REV"
git -C "$src" checkout -q --force --detach "$POLYKROM_REV"
git -C "$src" clean -q -fd crates apps
git -C "$src" apply "$here/polykrom-bench.patch"

(cd "$src" && CARGO_TARGET_DIR="$dir/target" cargo build --release -j "$JOBS" -p solidcraft-cli --bin polykrom-cli)

cli="$dir/target/release/polykrom-cli"
echo "polykrom-cli: $cli"
shasum -a 256 "$cli"

#!/usr/bin/env bash
# The OCCT reference program (RFC 0001): a frozen `nf` from Nightfall's final main, used only
# as a separate program through `nf kernel serve`. Nothing of it is linked into PolyCore or
# Polykrom. macOS on Apple silicon only (Nightfall's CMake presets fix arm64).
#
#   bench/oracle/build-nf.sh DIR [OCCT_INSTALL]
#
# DIR gets the Nightfall checkout and its build. OCCT_INSTALL is an existing OpenCASCADE 7.9.3
# install made by Nightfall's third_party/build-occt.sh at the same pins (its .nf-occt-commit
# stamp must match); without it the script builds OCCT itself, which takes a long time.
set -euo pipefail

NIGHTFALL_COMMIT=f4a8de07a639cef08b5127ea4894bbc1581658ce
OCCT_STAMP=a016080bf6738d6aeae020badee4e888ad1540a5-inc-align-d7887821ad973f2b2928fd4dcfabed5a4a4920d61a177634f865f9a8175054e4
JOBS=${JOBS:-4}

dir=${1:?usage: build-nf.sh DIR [OCCT_INSTALL]}
mkdir -p "$dir"
dir=$(cd "$dir" && pwd)
src="$dir/nightfall"

if [[ ! -d "$src/.git" ]]; then
  git init -q "$src"
  git -C "$src" remote add origin https://github.com/wckdboy/nightfall.git
fi
git -C "$src" fetch -q --depth 1 origin "$NIGHTFALL_COMMIT"
git -C "$src" checkout -q --detach "$NIGHTFALL_COMMIT"

occt=${2:-$dir/occt-7.9.3}
if [[ ! -f "$occt/.nf-occt-commit" ]]; then
  NF_OCCT_INSTALL_DIR="$occt" "$src/third_party/build-occt.sh" --jobs "$JOBS"
fi
if [[ "$(cat "$occt/.nf-occt-commit")" != "$OCCT_STAMP" ]]; then
  echo "OCCT at $occt has stamp $(cat "$occt/.nf-occt-commit"), want $OCCT_STAMP" >&2
  exit 1
fi

# No studies (Netgen, MFEM) and no tests: the oracle needs only `nf kernel serve`.
(cd "$src" && cmake --preset occt -DNF_WITH_STUDIES=OFF -DNF_BUILD_TESTS=OFF -DNF_OCCT_INSTALL_DIR="$occt")
(cd "$src" && CARGO_BUILD_JOBS="$JOBS" cmake --build --preset occt --target nf -j "$JOBS")

nf="$src/build/occt/tools/cli/nf"
echo "nf: $nf"
shasum -a 256 "$nf"

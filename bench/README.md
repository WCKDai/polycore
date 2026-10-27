# Bake-off and gate harness

PolyCore's standing score against OpenCASCADE, and the determinism check, as
[RFC 0001](../docs/rfcs/0001-polycore.md) sets them out. The latest result is
[scorecard.md](scorecard.md) ([JSON](scorecard.json)); each run's raw results are under
`results/<date>/`.

The bar is OCCT. Graded answers are the cases' own (analytic, hand-calculated or invariants);
OCCT is never the answer key. Nothing of OpenCASCADE or Nightfall is linked into PolyCore or
Polykrom: OCCT runs inside a frozen Nightfall `nf`, a separate program.

## The pieces

| File | What |
|---|---|
| `oracle/build-nf.sh` | Builds the OCCT reference program: Nightfall at its final `main`, `f4a8de07`, the `nf` target only (no studies, no tests), against OpenCASCADE 7.9.3 at Nightfall's pins. |
| `oracle/build-cli.sh` | Builds the bench's `polykrom-cli`: Polykrom at a pinned revision plus `oracle/polykrom-bench.patch`. |
| `oracle/polykrom-bench.patch` | Changes nothing unless its environment variables are set. It adds the Nightfall backend from Polykrom's unmerged `feat/nightfall-backend` branch (`SOLIDCRAFT_NIGHTFALL=<nf>` sends the features' joins, cuts, intersects, constant fillets, equal chamfers and inward shells to `nf kernel serve`); a strict mode (`POLYCORE_ORACLE_STRICT=1`: a refused operation fails instead of falling back to truck); a thread cap (`POLYCORE_THREADS`, with `RAYON_NUM_THREADS`); and three fields in each case's JSON: `oracle` (operations OpenCASCADE took and refused), `history` (faces whose names follow exact kernel history) and `geometry_hash` (FNV-1a over every body's exact native encoding). |
| `pins.json` | Commits and SHA-256 of both binaries. |
| `run.py` | Runs every kernel case and ladder case in one column, one process per case, with the case's timeout (180 s by default). |
| `score.py` | Scores the columns and writes the scorecard. |

## Columns

| Column | What runs |
|---|---|
| `truck` | Polykrom's kernel as it is: the code PolyCore grows from. |
| `occt` | The same Polykrom documents with OpenCASCADE behind the features' joins, cuts, intersects, constant fillets, equal chamfers and inward shells, strict. Extrudes, revolves, sweeps, lofts, primitives, variable and curved blends, and the kernel's internal booleans are still truck's. |
| `truck-1t`, `occt-1t` | The same on one thread, for the determinism check. |

Polykrom's cases are design recipes, so this is the only way to run them on OpenCASCADE today:
the `occt` column measures OCCT where Polykrom's documents reach it. Its measures (volume, area,
validity) are Polykrom's, taken on the body read back from OCCT's STEP.

## The rubric

Nightfall RFC 0001's weights and normalisation, with PolyCore RFC 0001's changes:

| Metric | Weight | Scored here as |
|---|---|---|
| Operation success | 30 % | Valid closed solids (every `valid`, `shells` and `watertight` check passes), or the clean refusal the case wants, ÷ cases attempted. Cases unsupported in both columns are left out. |
| Correctness | 20 % | Volume and area results within the comparison tolerance of the case's analytic answer ÷ such results measured. Invariant-only cases are not scored. |
| Operation history | 20 % | Faces whose names follow exact kernel history ÷ faces named. Nightfall scored this yes / partial / no; it is measured per face here. |
| Fillet and blend quality | 10 % | Operation success on `fillets/`, `ladder/07-fillets/` and `ladder/08-chamfers/`. |
| Performance | 10 % | 100 × the faster column's median time ÷ this column's, over the cases both built. Times are the case's commands, measured inside its process. |
| Robustness | 10 % | 40 for no crash and no hang, 30 for the determinism check, 30 for a green wasm32 build. Replaces "integration cost". |

Comparison tolerance: |measured − expected| ≤ 1e-6 · max(|expected|, scale), with scale *d³* for
volumes and *d²* for areas, *d* the diagonal of the case's expected bounding box (without one,
the scale is |expected| alone).

Determinism: a column and its one-thread run must give every case the same status, the same
geometry hash and the same measures, bit for bit.

## Running it

macOS on Apple silicon (the reference program's platform). One heavy build at a time.

```sh
JOBS=4 bench/oracle/build-nf.sh  /private/tmp/polycore-oracle [existing OCCT 7.9.3 install]
JOBS=4 bench/oracle/build-cli.sh /private/tmp/polycore-bench-build
NF=/private/tmp/polycore-oracle/nightfall/build/occt/tools/cli/nf
PK=/private/tmp/polycore-bench-build/polykrom
CLI=/private/tmp/polycore-bench-build/target/release/polykrom-cli
for col in truck occt truck-1t occt-1t; do
  python3 -I bench/run.py --polykrom $PK --cli $CLI --nf $NF --column $col --out runs/$col.json --jobs 3
done
python3 -I bench/score.py --polykrom $PK --runs runs --out bench --date YYYY-MM-DD --caveats runs/caveats.txt
```

`results/<date>/` keeps each run's columns (gzipped), the caveats the scorecard was written with,
and, for the baseline, a repeat of the occt column (`occt-repeat.json.gz`).

`build-nf.sh` reuses an existing OpenCASCADE install only when its `.nf-occt-commit` stamp
matches Nightfall's pins; without one it builds OCCT, which takes long.

#!/usr/bin/env python3
"""Run the kernel-case corpus and the ladder in one configuration ("column") of the bench.

    python3 -I bench/run.py --polykrom DIR --cli BIN --column NAME --out FILE [--nf BIN]
                            [--jobs N] [--timeout S] [--only SUBSTRING]

Columns:
  truck      Polykrom's kernel as it is (truck), all cores.
  truck-1t   The same on one thread (RAYON_NUM_THREADS=1, POLYCORE_THREADS=1).
  occt       OpenCASCADE behind the features' joins, cuts, intersects, constant fillets, equal
             chamfers and inward shells (`nf kernel serve`), strict: a refused operation fails
             instead of falling back to truck. Everything else is still truck.
  occt-1t    The same on one thread.

Every case runs in its own `polykrom-cli kernel-cases --json` process (so a crash or a hang is
that case's result), from the Polykrom checkout DIR, killed after its timeout. The output is one
JSON file: the column, its environment and every case's result.
"""

import argparse
import concurrent.futures
import json
import os
import subprocess
import sys
import time
from pathlib import Path

DEFAULT_TIMEOUT_S = 180
MAX_TIMEOUT_S = 1800


def cases(root):
    """Every case directory under tests/kernel-cases (ladder included), sorted."""
    base = root / "tests" / "kernel-cases"
    found = [p.parent for p in base.rglob("recipe.json") if (p.parent / "expected.json").is_file()]
    return sorted(found)


def environment(column, nf):
    env = {k: v for k, v in os.environ.items() if not k.startswith(("SOLIDCRAFT_NIGHTFALL", "POLYCORE_", "RAYON_", "RUST_LOG"))}
    extra = {}
    if column.startswith("occt"):
        if not nf:
            sys.exit("--nf is needed for an occt column")
        # RUST_LOG=warn: each refusal's reason (from `nf` or from the transfer) is logged.
        extra.update({"SOLIDCRAFT_NIGHTFALL": nf, "SOLIDCRAFT_NIGHTFALL_TIMEOUT": "60", "POLYCORE_ORACLE_STRICT": "1", "RUST_LOG": "warn"})
    if column.endswith("-1t"):
        extra.update({"RAYON_NUM_THREADS": "1", "POLYCORE_THREADS": "1"})
    env.update(extra)
    return env, extra


def run_case(cli, root, case, env, timeout_s):
    rel = case.relative_to(root)
    area = case.relative_to(root / "tests" / "kernel-cases").parts[0]
    path = str(case.relative_to(root / "tests" / "kernel-cases"))
    try:
        want = json.loads((case / "expected.json").read_text())
    except (OSError, ValueError):
        want = {}
    t = want.get("timeout_s")
    timeout = max(timeout_s, min(int(t), MAX_TIMEOUT_S)) if isinstance(t, (int, float)) else timeout_s
    start = time.monotonic()
    base = {"case": case.name, "area": area, "path": path}
    try:
        p = subprocess.run([cli, "kernel-cases", "--json", str(rel)], cwd=root, env=env, capture_output=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return {**base, "status": "hang", "error": f"no result after {timeout} s"}
    elapsed = time.monotonic() - start
    try:
        r = json.loads(p.stdout)[0]
    except (ValueError, IndexError, TypeError):
        tail = " / ".join(p.stderr.decode(errors="replace").strip().splitlines()[-3:])
        return {**base, "status": "crash", "error": f"process exited ({p.returncode}) without a result: {tail}"}
    r.update(base)
    r["process_seconds"] = elapsed
    reasons = [line.split("using truck: ", 1)[-1] for line in p.stderr.decode(errors="replace").splitlines() if "nightfall" in line and "refused" in line]
    if reasons:
        r["oracle_refusals"] = reasons
    return r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--polykrom", required=True, type=Path)
    ap.add_argument("--cli", required=True)
    ap.add_argument("--column", required=True, choices=["truck", "truck-1t", "occt", "occt-1t"])
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--nf")
    ap.add_argument("--jobs", type=int, default=3)
    ap.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT_S)
    ap.add_argument("--only")
    a = ap.parse_args()

    root = a.polykrom.resolve()
    cli = str(Path(a.cli).resolve())
    nf = str(Path(a.nf).resolve()) if a.nf else None
    env, extra = environment(a.column, nf)
    todo = [c for c in cases(root) if not a.only or a.only in str(c)]
    results = [None] * len(todo)
    started = time.time()
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, a.jobs)) as pool:
        futures = {pool.submit(run_case, cli, root, c, env, a.timeout): i for i, c in enumerate(todo)}
        for n, f in enumerate(concurrent.futures.as_completed(futures), 1):
            i = futures[f]
            results[i] = f.result()
            print(f"[{n}/{len(todo)}] {results[i]['status']:<11} {results[i]['path']}", flush=True)
    rev = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True).stdout.strip()
    out = {
        "column": a.column,
        "environment": {k: (Path(v).name if k == "SOLIDCRAFT_NIGHTFALL" else v) for k, v in extra.items()},
        "polykrom": rev,
        "jobs": a.jobs,
        "timeout_s": a.timeout,
        "started": time.strftime("%Y-%m-%dT%H:%M:%S%z", time.localtime(started)),
        "wall_seconds": time.time() - started,
        "load_average": os.getloadavg(),
        "results": results,
    }
    a.out.parent.mkdir(parents=True, exist_ok=True)
    a.out.write_text(json.dumps(out, indent=1))
    print(f"{a.column}: {len(results)} cases in {out['wall_seconds']:.0f} s -> {a.out}")


if __name__ == "__main__":
    main()

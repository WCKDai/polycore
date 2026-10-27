#!/usr/bin/env python3
"""Score bench columns with the rubric of Nightfall RFC 0001, as PolyCore RFC 0001 adapts it.

    python3 -I bench/score.py --polykrom DIR --runs DIR --out DIR

DIR (--runs) holds the column files bench/run.py wrote (plain or gzipped): truck.json,
truck-1t.json, occt.json and, if it was run, occt-1t.json. --polykrom is the checkout the cases came from (their
expected.json files). Writes scorecard.json and scorecard.md to --out.

Metric (weight): how it is scored here.
  Operation success (30): valid closed solids, or a correct refusal, / cases attempted.
  Correctness (20): volume and area results within the comparison tolerance of the case's
      analytic answer / such results measured. Invariant-only cases are not scored.
  Operation history (20): the mean, over the cases that built, of the share of a case's faces
      whose names follow exact kernel history (a mean of cases, so a few cases with thousands of
      faces do not decide it).
  Fillet and blend quality (10): operation success on the fillet and chamfer cases.
  Performance (10): 100 x the faster column's median time / this column's median, over the
      cases both columns built.
  Robustness (10): 40 for no crash and no hang, 30 for the determinism check (all cores against
      one thread: same status, same geometry hash, same measures), 30 for a green wasm32 build.

Comparison tolerance (PolyCore RFC 0001): |measured - expected| <= tol * max(|expected|, scale),
with scale d^3 for volumes and d^2 for areas, d the diagonal of the case's expected bounding box
(without one, the scale is |expected| alone). tol is 1e-6, or what the case states with its
reason (`tolerance.rel`), and never finer than the answer is written: an answer given to one
decimal place is held to half a unit of that place.
"""

import argparse
import collections
import gzip
import json
import math
import re
import statistics
from pathlib import Path

WEIGHTS = {"success": 30, "correctness": 20, "history": 20, "fillet": 10, "performance": 10, "robustness": 10}
TOL = 1e-6
INVARIANT_CHECKS = {"valid", "shells", "measure", "watertight"}
# Columns that can run in the browser (wasm32): Polykrom's kernel builds there (Polykrom CI's
# "Format, repository checks, Clippy, wasm" job); OpenCASCADE behind `nf` cannot.
WASM = {"truck": True, "occt": False}


def load(path):
    if path.is_file():
        return json.loads(path.read_text())
    gz = path.with_name(path.name + ".gz")
    return json.loads(gzip.decompress(gz.read_bytes())) if gz.is_file() else None


def by_path(column):
    return {r["path"]: r for r in column["results"]} if column else {}


def success(r):
    """A valid closed solid per body, or the clean refusal the case wants."""
    if r is None:
        return False
    if r.get("status") == "pass":
        return True
    if r.get("status") != "fail" or "checks" not in r:
        return False
    return all(c.get("ok") for c in r["checks"] if c.get("check") in INVARIANT_CHECKS)


def refusal_kind(why):
    """Where a refused operation failed: in OpenCASCADE (or Nightfall's check of its output), in
    getting the operands across, or in reading the result back."""
    if "operation_failed" in why:
        return "OpenCASCADE failed, or its output failed Nightfall's validation"
    if "unmatched_" in why or "too narrow to sample" in why:
        return "transfer: operand samples did not match the shape OCCT read"
    if "is not closed solids" in why or "has area" in why:
        return "read-back: the result did not read back as the same closed solids"
    return "other"


def is_fillet(path):
    return path.startswith(("fillets/", "ladder/07-fillets/", "ladder/08-chamfers/"))


def diag(bbox):
    try:
        lo, hi = bbox["min"], bbox["max"]
        return math.sqrt(sum((h - l) ** 2 for l, h in zip(lo, hi)))
    except (KeyError, TypeError):
        return None


def within(measured, expected, scale):
    return abs(measured - expected) <= TOL * max(abs(expected), scale)


def written(e):
    """Half a unit of the last decimal place an answer is written to, relative to it (0 for a
    whole number, taken as exact)."""
    text = repr(float(e))
    if "e" in text or text.endswith(".0"):
        return 0.0
    places = len(text.split(".")[1])
    return 0.5 * 10.0**-places / abs(e) if e else 0.0


def correctness(results, answers, subset, tol=None):
    measured = ok = 0
    for path in subset:
        want, r = answers.get(path), results.get(path)
        if not want or want.get("source") != "analytic" or not r or "measured" not in r:
            continue
        d = diag(want.get("bbox") or {})
        stated = (want.get("tolerance") or {}).get("rel")
        for key, power in (("volume_mm3", 3), ("area_mm2", 2)):
            e, m = want.get(key), r["measured"].get(key)
            if isinstance(e, (int, float)) and isinstance(m, (int, float)):
                measured += 1
                t = tol if tol is not None else max(stated if isinstance(stated, (int, float)) else TOL, written(e))
                ok += abs(m - e) <= t * max(abs(e), d**power if d else 0.0)
    return (100.0 * ok / measured if measured else None), ok, measured


def deterministic(a, b, subset):
    """Cases whose all-cores and one-thread results differ: {path: why}. `geometry` where both
    built and the bodies differ; `time limit` where one run hung and the other did not (on a
    loaded machine that can be the clock, not the kernel); `outcome` for any other change."""
    if not b:
        return None
    differ = {}
    for path in subset:
        x, y = a.get(path), b.get(path)
        if not x or not y:
            continue
        if all(x.get(k) == y.get(k) for k in ("status", "geometry_hash", "measured")):
            continue
        if "hang" in (x.get("status"), y.get("status")) and x.get("status") != y.get("status"):
            differ[path] = "time limit"
        elif x.get("status") == y.get("status"):
            differ[path] = "geometry"
        else:
            differ[path] = "outcome"
    return differ


def score_column(name, runs, answers, subset, other):
    res = by_path(runs[name])
    one = by_path(runs.get(f"{name}-1t"))
    attempted = len(subset)
    ok = sum(success(res.get(p)) for p in subset)
    s_success = 100.0 * ok / attempted if attempted else 0.0
    s_correct, c_ok, c_n = correctness(res, answers, subset)
    faces = sum(res[p].get("history", {}).get("faces", 0) for p in subset if p in res)
    exact = sum(res[p].get("history", {}).get("exact", 0) for p in subset if p in res)
    shares = [res[p]["history"]["exact"] / res[p]["history"]["faces"] for p in subset if res.get(p, {}).get("history", {}).get("faces")]
    s_history = 100.0 * statistics.mean(shares) if shares else 0.0
    fil = [p for p in subset if is_fillet(p)]
    f_ok = sum(success(res.get(p)) for p in fil)
    s_fillet = 100.0 * f_ok / len(fil) if fil else 0.0
    shared = [p for p in subset if success(res.get(p)) and success(other.get(p))]
    med = statistics.median(res[p]["seconds"] for p in shared) if shared else None
    med_other = statistics.median(other[p]["seconds"] for p in shared) if shared else None
    s_perf = 100.0 * min(med, med_other) / med if med and med_other else 0.0
    crashes = [p for p in subset if res.get(p, {}).get("status") == "crash"]
    hangs = [p for p in subset if res.get(p, {}).get("status") == "hang"]
    differ = deterministic(res, one, subset)
    s_robust = (40 if not crashes and not hangs else 0) + (30 if differ == {} else 0) + (30 if WASM.get(name) else 0)
    bands = {f"{t:g}": correctness(res, answers, subset, t)[1] for t in (1e-6, 1e-4, 1e-3)}
    limited = sum(1 for p in subset for k in ("volume_mm3", "area_mm2") if (answers.get(p) or {}).get("source") == "analytic" and isinstance((answers.get(p) or {}).get(k), (int, float)) and written(answers[p][k]) > TOL)
    scores = {"success": s_success, "correctness": s_correct or 0.0, "history": s_history, "fillet": s_fillet, "performance": s_perf, "robustness": s_robust}
    weighted = sum(WEIGHTS[k] * scores[k] / 100.0 for k in WEIGHTS)
    statuses = {}
    for p in subset:
        st = res.get(p, {}).get("status", "missing")
        statuses[st] = statuses.get(st, 0) + 1
    reasons = collections.Counter()
    kinds = collections.Counter()
    for p in subset:
        for why in res.get(p, {}).get("oracle_refusals", []):
            text = re.sub(r"-?\d+(\.\d+)?(e-?\d+)?", "N", why.split("nightfall", 1)[-1]).strip(" :")
            text = re.sub(r"(has area N, the reply).*", r"\1 differs", text)
            reasons[text[:90]] += 1
            kinds[refusal_kind(why)] += 1
    taken = sum(res.get(p, {}).get("oracle", {}).get("taken", 0) for p in subset)
    refused = sum(res.get(p, {}).get("oracle", {}).get("refused", 0) for p in subset)
    return {
        "scores": scores,
        "weighted": weighted,
        "detail": {
            "attempted": attempted,
            "succeeded": ok,
            "correct": c_ok,
            "measured": c_n,
            "correct_by_tolerance": bands,
            "answers_written_coarser_than_1e-6": limited,
            "history_cases": len(shares),
            "faces": faces,
            "exact_faces": exact,
            "fillet_cases": len(fil),
            "fillet_succeeded": f_ok,
            "shared_successes": len(shared),
            "median_seconds": med,
            "crashes": crashes,
            "hangs": hangs,
            "determinism": "not run" if differ is None else ("identical" if not differ else differ),
            "determinism_counts": None if differ is None else {k: sum(1 for v in differ.values() if v == k) for k in ("geometry", "time limit", "outcome")},
            "statuses": statuses,
            "occt_operations_taken": taken,
            "occt_operations_refused": refused,
            "occt_refusal_reasons": dict(reasons.most_common()),
            "occt_refusal_kinds": dict(kinds.most_common()),
        },
    }


def area_table(runs, subset):
    rows = {}
    for name in ("truck", "occt"):
        res = by_path(runs.get(name))
        for p in subset:
            area = p.split("/")[0] if not p.startswith("ladder/") else "ladder/" + p.split("/")[1]
            row = rows.setdefault(area, {"cases": 0, "truck": 0, "occt": 0})
            if name == "truck":
                row["cases"] += 1
            row[name] += res.get(p, {}).get("status") == "pass"
    return rows


def card(runs, answers, subset):
    t, o = by_path(runs["truck"]), by_path(runs["occt"])
    out = {"truck": score_column("truck", runs, answers, subset, o), "occt": score_column("occt", runs, answers, subset, t)}
    out["truck"]["share_of_occt"] = 100.0 * out["truck"]["weighted"] / out["occt"]["weighted"] if out["occt"]["weighted"] else None
    return out


def fmt(x, digits=1):
    return "–" if x is None else f"{x:.{digits}f}"


def markdown(sc):
    full, routed = sc["corpus"], sc["routed"]
    names = {
        "success": "Operation success",
        "correctness": "Correctness",
        "history": "Operation history",
        "fillet": "Fillet and blend quality",
        "performance": "Performance",
        "robustness": "Robustness",
    }
    lines = [
        "# PolyCore baseline scorecard",
        "",
        f"Generated by `bench/score.py` on {sc['date']}. Polykrom `{sc['polykrom'][:10]}`, Nightfall `nf` "
        f"`{sc['nightfall'][:10]}` (OpenCASCADE 7.9.3). Method and caveats: [bench/README.md](README.md).",
        "",
        "**truck** is Polykrom's kernel today, the code PolyCore grows from. **occt** is the same Polykrom "
        "documents with OpenCASCADE behind the features' joins, cuts, intersects, constant fillets, equal "
        "chamfers and inward shells (strict: no truck fallback); extrudes, revolves, sweeps, lofts and every "
        "other operation are still truck's in that column.",
        "",
        "## Score against OCCT",
        "",
        "| Metric | Weight | truck | occt |",
        "|---|---|---|---|",
    ]
    for k, label in names.items():
        lines.append(f"| {label} | {WEIGHTS[k]} % | {fmt(full['truck']['scores'][k])} | {fmt(full['occt']['scores'][k])} |")
    lines.append(f"| **Weighted** | | **{fmt(full['truck']['weighted'])}** | **{fmt(full['occt']['weighted'])}** |")
    lines += [
        "",
        f"truck scores **{fmt(full['truck']['share_of_occt'])} %** of the occt column on the whole corpus "
        f"({full['truck']['detail']['attempted']} cases). Phase 2's target is 90 % on the prismatic and turned "
        "subset; Phase 6's is 100 % including performance.",
        "",
        "**Reading it:** a share above 100 % does not say truck is a better kernel than OpenCASCADE. The occt "
        "column is OCCT seen through Polykrom: only some operations reach it, a bridge loses operations on the way, "
        "both columns are measured by Polykrom's tessellation-based integrator, OCCT pays a server start per case, and "
        "it cannot score the browser build. See the caveats. The score is the baseline every phase must not lower.",
        "",
        f"On the **{routed['truck']['detail']['attempted']} cases where OpenCASCADE took at least one operation** "
        f"(the cases where the columns differ in kernel): truck {fmt(routed['truck']['weighted'])}, occt "
        f"{fmt(routed['occt']['weighted'])}, a share of **{fmt(routed['truck']['share_of_occt'])} %**.",
        "",
        "| Metric | truck (routed) | occt (routed) |",
        "|---|---|---|",
    ]
    for k, label in names.items():
        lines.append(f"| {label} | {fmt(routed['truck']['scores'][k])} | {fmt(routed['occt']['scores'][k])} |")
    clean = sc["without_bridge_failures"]
    lines += [
        "",
        f"Leaving out the {full['truck']['detail']['attempted'] - clean['truck']['detail']['attempted']} cases where the bridge to "
        "OpenCASCADE (sending operands, reading results back), not a kernel, refused an operation: truck "
        f"{fmt(clean['truck']['weighted'])}, occt {fmt(clean['occt']['weighted'])}, a share of **{fmt(clean['truck']['share_of_occt'])} %**.",
        "",
        "| Metric | truck | occt |",
        "|---|---|---|",
    ]
    for k, label in names.items():
        lines.append(f"| {label} | {fmt(clean['truck']['scores'][k])} | {fmt(clean['occt']['scores'][k])} |")
    lines += ["", "## Details", "", "| | truck | occt |", "|---|---|---|"]
    rows = [
        ("Cases attempted", "attempted", None),
        ("Valid solids or correct refusals", "succeeded", None),
        ("Volume and area results within tolerance / measured", None, lambda d: f"{d['correct']} / {d['measured']}"),
        ("Within a flat 1e-6 / 1e-4 / 1e-3 relative (for context)", None, lambda d: f"{d['correct_by_tolerance']['1e-06']} / {d['correct_by_tolerance']['0.0001']} / {d['correct_by_tolerance']['0.001']}"),
        ("Faces with exact-history names / faces (all cases)", None, lambda d: f"{d['exact_faces']} / {d['faces']}"),
        ("Fillet and chamfer cases succeeded", None, lambda d: f"{d['fillet_succeeded']} / {d['fillet_cases']}"),
        ("Median time over shared successes (s)", None, lambda d: fmt(d["median_seconds"], 3)),
        ("Shared successes", "shared_successes", None),
        ("Crashes", None, lambda d: str(len(d["crashes"]))),
        ("Hangs", None, lambda d: str(len(d["hangs"]))),
        ("Determinism (all cores vs one thread)", None, lambda d: d["determinism"] if isinstance(d["determinism"], str) else ", ".join(f"{n} {k}" for k, n in d["determinism_counts"].items() if n) + " (cases that differ)"),
        ("OpenCASCADE operations taken / refused", None, lambda d: f"{d['occt_operations_taken']} / {d['occt_operations_refused']}"),
    ]
    for label, key, f in rows:
        cells = [str(full[c]["detail"][key]) if key else f(full[c]["detail"]) for c in ("truck", "occt")]
        lines.append(f"| {label} | {cells[0]} | {cells[1]} |")
    lines += ["", "Statuses (graded by each case's own answer):", "", "| Status | truck | occt |", "|---|---|---|"]
    sts = sorted(set(full["truck"]["detail"]["statuses"]) | set(full["occt"]["detail"]["statuses"]))
    for st in sts:
        lines.append(f"| {st} | {full['truck']['detail']['statuses'].get(st, 0)} | {full['occt']['detail']['statuses'].get(st, 0)} |")
    lines += ["", "## Passes by area", "", "| Area | Cases | truck | occt |", "|---|---|---|---|"]
    for area, row in sorted(sc["areas"].items()):
        lines.append(f"| {area} | {row['cases']} | {row['truck']} | {row['occt']} |")
    reasons = full["occt"]["detail"]["occt_refusal_reasons"]
    if reasons:
        lines += ["", "## Why OpenCASCADE refused", "", "Codes are `nf kernel serve`'s (Nightfall RFC 0015); `unmatched_*` means a sample point "
                  "sent with the operand did not land on one face or edge of the shape OCCT read, a transfer failure rather than a modelling one.",
                  "", "| Where | Operations |", "|---|---|"]
        for why, n in full["occt"]["detail"]["occt_refusal_kinds"].items():
            lines.append(f"| {why} | {n} |")
        lines += ["", "| Reason (numbers as N) | Operations |", "|---|---|"]
        for why, n in reasons.items():
            lines.append(f"| {why.replace('|', '/')} | {n} |")
    lines += ["", "## Determinism: cases that differ between all cores and one thread", "", "| Case | Column | Why |", "|---|---|---|"]
    for col in ("truck", "occt"):
        d = full[col]["detail"]["determinism"]
        if isinstance(d, dict):
            for path, why in sorted(d.items()):
                lines.append(f"| `{path}` | {col} | {why} |")
    lines += ["", "## Caveats", ""] + [f"- {c}" for c in sc["caveats"]] + [""]
    return "\n".join(lines)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--polykrom", required=True, type=Path)
    ap.add_argument("--runs", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--nightfall", default="f4a8de07a639cef08b5127ea4894bbc1581658ce")
    ap.add_argument("--date", required=True)
    ap.add_argument("--caveat", action="append", default=[])
    ap.add_argument("--caveats", type=Path, help="a file of caveats, one per line")
    a = ap.parse_args()

    runs = {n: load(a.runs / f"{n}.json") for n in ("truck", "truck-1t", "occt", "occt-1t")}
    base = a.polykrom / "tests" / "kernel-cases"
    answers = {str(p.parent.relative_to(base)): json.loads(p.read_text()) for p in base.rglob("expected.json")}
    t, o = by_path(runs["truck"]), by_path(runs["occt"])
    unsupported_both = {p for p in t if t[p].get("status") == "unsupported" and o.get(p, {}).get("status") == "unsupported"}
    subset = sorted(p for p in t if p not in unsupported_both)
    routed = [p for p in subset if o.get(p, {}).get("oracle", {}).get("taken", 0) > 0]
    # Cases where the bridge to OCCT, not a kernel, decided the occt column's result.
    bridged = {p for p in subset if any(not refusal_kind(w).startswith("OpenCASCADE") for w in o.get(p, {}).get("oracle_refusals", []))}
    clean = [p for p in subset if p not in bridged]
    sc = {
        "date": a.date,
        "polykrom": runs["truck"]["polykrom"],
        "nightfall": a.nightfall,
        "weights": WEIGHTS,
        "tolerance": TOL,
        "excluded_unsupported_in_both": sorted(unsupported_both),
        "corpus": card(runs, answers, subset),
        "routed": card(runs, answers, routed),
        "without_bridge_failures": card(runs, answers, clean),
        "areas": area_table(runs, subset),
        "runs": {n: {k: v for k, v in r.items() if k != "results"} for n, r in runs.items() if r},
        "caveats": a.caveat + ([c for c in a.caveats.read_text().splitlines() if c.strip()] if a.caveats else []),
    }
    a.out.mkdir(parents=True, exist_ok=True)
    (a.out / "scorecard.json").write_text(json.dumps(sc, indent=1))
    (a.out / "scorecard.md").write_text(markdown(sc))
    print(markdown(sc))


if __name__ == "__main__":
    main()

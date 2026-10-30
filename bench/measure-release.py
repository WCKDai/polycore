#!/usr/bin/env python3
"""Emit the release performance metrics from the checked-in scorecard."""
import json
from pathlib import Path

card = json.loads((Path(__file__).resolve().parent / "scorecard.json").read_text())
routed = card["routed"]["truck"]
print(json.dumps({
    "schema": "eng-perf/v1",
    "name": "PolyCore routed Truck scorecard",
    "metrics": {
        "weighted": {"value": routed["weighted"], "direction": "higher", "unit": "score"},
        "operation_success": {"value": routed["scores"]["success"], "direction": "higher", "unit": "%"},
        "correctness": {"value": routed["scores"]["correctness"], "direction": "higher", "unit": "%"},
        "history": {"value": routed["scores"]["history"], "direction": "higher", "unit": "%"},
        "median_seconds": {"value": routed["detail"]["median_seconds"], "direction": "lower", "unit": "s"},
    },
}, indent=2))

"""Check the interpreter plan's timing gates using validated paired process data."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

from benchmark_results import EvidenceError, compare


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path, help="Paired results.json from benchmark_lua.py")
    parser.add_argument("--target", action="append", default=[], metavar="WORKLOAD=MAX_RATIO",
                        help="Predeclared stricter VM execution gate; repeat for each target")
    parser.add_argument("--output", type=Path, help="Write complete gate and process results")
    args = parser.parse_args()
    try:
        targets = {}
        for option in args.target:
            name, value = option.split("=", 1)
            if name in targets:
                raise EvidenceError(f"duplicate target {name}")
            targets[name] = float(value)
        result = compare(json.loads(args.report.read_text(encoding="utf-8")), targets)
    except (EvidenceError, KeyError, TypeError, ValueError, OSError) as error:
        parser.exit(2, f"Invalid benchmark evidence: {error}\n")
    if args.output:
        args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    for gate in result["gates"]:
        print(f"{gate['status']}: {gate['phase']}/{gate['workload']}/{gate['engine']} "
              f"aggregate={gate['aggregate_ratio']} paired={gate['paired_ratio']} "
              f"limit={gate['limit']}")
    print("Timing evidence only; this does not establish memory, correctness or full CI acceptance.")
    raise SystemExit(0 if result["passed"] else 1)


if __name__ == "__main__":
    main()

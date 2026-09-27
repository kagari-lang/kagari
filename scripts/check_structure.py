# /// script
# requires-python = ">=3.11"
# dependencies = ["tree-sitter==0.25.2", "tree-sitter-rust==0.24.0"]
# ///
"""Check Rust source structure without building the workspace."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
import unittest

from structure_check.rust import check_sources
from structure_check.exceptions import apply_exceptions


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--json", action="store_true", help="Emit a machine-readable report")
    parser.add_argument("--self-test", action="store_true", help="Run checker regression tests")
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.defaultTestLoader.discover(str(Path(__file__).parent / "tests"))
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    root = args.root.resolve()
    try:
        result = subprocess.run(
            ["git", "-C", str(root), "ls-files", "--cached", "--others",
             "--exclude-standard", "-z"],
            check=True, capture_output=True,
        )
        paths = sorted({Path(p.decode("utf-8")) for p in result.stdout.split(b"\0")
                        if p.endswith(b".rs")})
        sources = {p.as_posix(): (root / p).read_bytes() for p in paths if (root / p).is_file()}
        manifests = {
            p.as_posix(): (root / p).read_bytes()
            for p in {Path(n.decode("utf-8")) for n in result.stdout.split(b"\0")
                      if n.endswith(b"Cargo.toml")}
            if (root / p).is_file()
        }
        report = apply_exceptions(check_sources(sources, manifests), root)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"structure-check: {error}", file=sys.stderr)
        return 2
    if args.json:
        print(json.dumps(report, indent=2, ensure_ascii=False))
    else:
        for finding in report["findings"]:
            print(f"{finding['path']}:{finding['line']}:{finding['column']}: "
                  f"{finding['rule']}: {finding['message']}")
        for exception in report["exceptions"]:
            print(f"{exception['path']}:{exception['line']}: exception: "
                  f"{exception['rule']}: {exception['reason']} "
                  f"(evidence: {exception['evidence']})")
        print(f"Checked {report['files']} Rust files; {len(report['findings'])} violations; "
              f"{len(report['exceptions'])} documented exceptions.")
    return 1 if report["findings"] else 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    raise SystemExit(main())

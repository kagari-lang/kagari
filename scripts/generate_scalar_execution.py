"""Regenerate the three committed scalar instruction regions, or check for drift."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
from string import Template
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
CATALOG = Path("crates/kagari-runtime/codegen/scalar-instructions.toml")
TARGETS = {
    "scalar-variants": Path("crates/kagari-runtime/src/module/execution/instruction.rs"),
    "scalar-preparation": Path("crates/kagari-runtime/src/module/execution/scalar.rs"),
    "scalar-execution": Path("crates/kagari-runtime/src/frame/cursor/scalars/execute.rs"),
}
FIELDS = {
    "binary": ("source_pattern", "rr", "rk", "immediate_type", "helper"),
    "shift": ("kind", "rr", "rk", "helper"),
    "compare": ("domain", "rr", "rk", "branch_rr", "branch_rk", "immediate_type", "helper"),
    "simple_compare": ("domain", "rr", "branch_rr", "helper"),
    "unary": ("operation", "variant", "helper"),
    "convert": ("operation", "source_type", "target_type"),
}
VARIANT_FIELDS = {"rr", "rk", "branch_rr", "branch_rk", "variant"}

# Fixed family renderers. Rust owns the semantics and type-checks the emitted code.
TEMPLATES = {
    'scalar-variants': [
        ('binary', """\
$rr { dst: ScalarSlot, lhs: ScalarSlot, rhs: ScalarSlot },
$rk { dst: ScalarSlot, lhs: ScalarSlot, rhs: $immediate_type },
"""),
        ('shift', """\
$rr { dst: ScalarSlot, lhs: ScalarSlot, rhs: ScalarSlot, count_domain: IntegerType },
$rk { dst: ScalarSlot, lhs: ScalarSlot, rhs: u64, count_domain: IntegerType },
"""),
        ('compare', """\
$rr { dst: ScalarSlot, lhs: ScalarSlot, rhs: ScalarSlot, condition: Comparison },
$rk { dst: ScalarSlot, lhs: ScalarSlot, rhs: $immediate_type, condition: Comparison },
$branch_rr { lhs: ScalarSlot, rhs: ScalarSlot, then_target: JumpTarget, else_target: JumpTarget, condition: Comparison },
$branch_rk { lhs: ScalarSlot, rhs: $immediate_type, then_target: JumpTarget, else_target: JumpTarget, condition: Comparison },
"""),
        ('simple_compare', """\
$rr { dst: ScalarSlot, lhs: ScalarSlot, rhs: ScalarSlot, condition: Comparison },
$branch_rr { lhs: ScalarSlot, rhs: ScalarSlot, then_target: JumpTarget, else_target: JumpTarget, condition: Comparison },
"""),
        ('unary', """\
$variant { dst: ScalarSlot, src: ScalarSlot },
"""),
        ('convert', """\
$operation { dst: ScalarSlot, src: ScalarSlot, source: $source_type, target: $target_type },
"""),
    ],
    'scalar-preparation': [
        ('binary', """\
($source_pattern, ScalarInput::Slot(lhs), ScalarInput::Slot(rhs), ScalarTarget::Store(dst)) => Some(Self::$rr { dst, lhs, rhs }),
($source_pattern, ScalarInput::Slot(lhs), ScalarInput::Constant(rhs), ScalarTarget::Store(dst)) => Some(Self::$rk { dst, lhs, $prepared_rhs }),
"""),
        ('shift', """\
(ScalarOperation::Shift { kind: ShiftKind::$kind, count_domain }, ScalarInput::Slot(lhs), ScalarInput::Slot(rhs), ScalarTarget::Store(dst)) => Some(Self::$rr { dst, lhs, rhs, count_domain }),
(ScalarOperation::Shift { kind: ShiftKind::$kind, count_domain }, ScalarInput::Slot(lhs), ScalarInput::Constant(rhs), ScalarTarget::Store(dst)) => Some(Self::$rk { dst, lhs, rhs, count_domain }),
"""),
        ('compare', """\
(ScalarOperation::Compare { domain: ComparisonDomain::$domain, condition }, ScalarInput::Slot(lhs), ScalarInput::Slot(rhs), ScalarTarget::Store(dst)) => Some(Self::$rr { dst, lhs, rhs, condition }),
(ScalarOperation::Compare { domain: ComparisonDomain::$domain, condition }, ScalarInput::Slot(lhs), ScalarInput::Constant(rhs), ScalarTarget::Store(dst)) => Some(Self::$rk { dst, lhs, $prepared_rhs, condition }),
(ScalarOperation::Compare { domain: ComparisonDomain::$domain, condition }, ScalarInput::Slot(lhs), ScalarInput::Slot(rhs), ScalarTarget::Branch { then_target, else_target }) => Some(Self::$branch_rr { lhs, rhs, then_target, else_target, condition }),
(ScalarOperation::Compare { domain: ComparisonDomain::$domain, condition }, ScalarInput::Slot(lhs), ScalarInput::Constant(rhs), ScalarTarget::Branch { then_target, else_target }) => Some(Self::$branch_rk { lhs, $prepared_rhs, then_target, else_target, condition }),
"""),
        ('simple_compare', """\
(ScalarOperation::Compare { domain: ComparisonDomain::$domain, condition }, ScalarInput::Slot(lhs), ScalarInput::Slot(rhs), ScalarTarget::Store(dst)) => Some(Self::$rr { dst, lhs, rhs, condition }),
(ScalarOperation::Compare { domain: ComparisonDomain::$domain, condition }, ScalarInput::Slot(lhs), ScalarInput::Slot(rhs), ScalarTarget::Branch { then_target, else_target }) => Some(Self::$branch_rr { lhs, rhs, then_target, else_target, condition }),
"""),
        ('unary', """\
(ScalarOperation::$operation, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => Some(Self::$variant { dst, src }),
"""),
        ('convert', """\
(ScalarOperation::$operation { source, target }, ScalarInput::Slot(src), _, ScalarTarget::Store(dst)) => Some(Self::$operation { dst, src, source, target }),
"""),
    ],
    'scalar-execution': [
        ('binary', """\
ExecutionInstruction::$rr { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (dst, attempt!($helper(lhs, rhs).map_err(ScalarFailure::Trap)))
                },
"""),
        ('binary', """\
ExecutionInstruction::$rk { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    $payload_binding
                    (dst, attempt!($helper(lhs, rhs).map_err(ScalarFailure::Trap)))
                },
"""),
        ('shift', """\
ExecutionInstruction::$rr { dst, lhs, rhs, count_domain } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (dst, attempt!($helper(lhs, rhs, count_domain).map_err(ScalarFailure::Trap)))
                },
"""),
        ('shift', """\
ExecutionInstruction::$rk { dst, lhs, rhs, count_domain } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    $payload_binding
                    (dst, attempt!($helper(lhs, rhs, count_domain).map_err(ScalarFailure::Trap)))
                },
"""),
        ('compare', """\
ExecutionInstruction::$rr { dst, lhs, rhs, condition } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (dst, attempt!($helper(lhs, rhs, condition).map_err(ScalarFailure::Trap)))
                },
"""),
        ('compare', """\
ExecutionInstruction::$rk { dst, lhs, rhs, condition } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    $payload_binding
                    (dst, attempt!($helper(lhs, rhs, condition).map_err(ScalarFailure::Trap)))
                },
"""),
        ('simple_compare', """\
ExecutionInstruction::$rr { dst, lhs, rhs, condition } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (dst, attempt!($helper(lhs, rhs, condition).map_err(ScalarFailure::Trap)))
                },
"""),
        ('compare', """\
ExecutionInstruction::$branch_rr { lhs, rhs, condition, then_target, else_target } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let value = attempt!($helper(lhs, rhs, condition).map_err(ScalarFailure::Trap));
                    next = attempt!(self.jump(if value == 1 { then_target.index() } else { else_target.index() }));
                    continue;
                },
"""),
        ('compare', """\
ExecutionInstruction::$branch_rk { lhs, rhs, condition, then_target, else_target } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    $payload_binding
                    let value = attempt!($helper(lhs, rhs, condition).map_err(ScalarFailure::Trap));
                    next = attempt!(self.jump(if value == 1 { then_target.index() } else { else_target.index() }));
                    continue;
                },
"""),
        ('simple_compare', """\
ExecutionInstruction::$branch_rr { lhs, rhs, condition, then_target, else_target } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let value = attempt!($helper(lhs, rhs, condition).map_err(ScalarFailure::Trap));
                    next = attempt!(self.jump(if value == 1 { then_target.index() } else { else_target.index() }));
                    continue;
                },
"""),
        ('unary', """\
ExecutionInstruction::$variant { dst, src } => {
                    let value = attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (dst, attempt!($helper(value, value).map_err(ScalarFailure::Trap)))
                },
"""),
        ('convert', """\
ExecutionInstruction::$operation { dst, src, source, target } => {
                    let value = attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (dst, attempt!(payload::convert(value, source.builtin_type(), target.builtin_type()).map_err(ScalarFailure::Trap)))
                },
"""),
    ],
}


def load_catalog(path: Path) -> dict[str, list[dict[str, str]]]:
    with path.open("rb") as source:
        catalog = tomllib.load(source)
    if set(catalog) != set(FIELDS):
        raise ValueError("catalog must contain exactly the six scalar families")
    variants: set[str] = set()
    operations: set[tuple[str, str]] = set()
    for family, fields in FIELDS.items():
        rows = catalog[family]
        if not isinstance(rows, list) or not rows:
            raise ValueError(f"{family}: expected a nonempty array of instruction rows")
        for row in rows:
            if not isinstance(row, dict) or set(row) != set(fields):
                raise ValueError(f"{family}: expected fields {fields}")
            if any(not isinstance(value, str) or not value.strip() or "\n" in value
                   or "\r" in value for value in row.values()):
                raise ValueError(f"{family}: fields must be nonempty single-line strings")
            operation = (family, row[fields[0]])
            if operation in operations:
                raise ValueError(f"{family}: duplicate source operation {operation[1]}")
            operations.add(operation)
            for field in VARIANT_FIELDS.intersection(row):
                variant = row[field]
                if not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", variant) or variant in variants:
                    raise ValueError(f"invalid or duplicate variant: {variant}")
                variants.add(variant)
            if family == "convert":
                variant = row["operation"]
                if not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", variant) or variant in variants:
                    raise ValueError(f"invalid or duplicate variant: {variant}")
                variants.add(variant)
    return catalog


def render(catalog: dict[str, list[dict[str, str]]], region: str) -> str:
    arms = []
    for family, template in TEMPLATES[region]:
        for row in catalog[family]:
            immediate = row.get("immediate_type", "u64")
            values = dict(row)
            values["prepared_rhs"] = "rhs" if immediate == "u64" else f"rhs: rhs as {immediate}"
            values["payload_rhs"] = ("rhs" if immediate == "u64" else
                                     "rhs as u64" if immediate == "i64" else
                                     "rhs as i64 as u64")
            payload_rhs = values["payload_rhs"]
            values["payload_binding"] = "" if immediate == "u64" else f"let rhs = {payload_rhs};"
            arms.append(Template(template).substitute(values).strip())
    return ("// Source: codegen/scalar-instructions.toml. Regenerate with:\n"
            "// uv run python scripts/generate_scalar_execution.py\n" + "\n".join(arms))


def replace_region(source: str, region: str, generated: str) -> str:
    markers = list(re.finditer(r"(?m)^[ \t]*// (BEGIN|END) GENERATED ([\w-]+)[ \t]*$", source))
    if [(match[1], match[2]) for match in markers] != [("BEGIN", region), ("END", region)]:
        raise ValueError(f"{region}: expected one non-nested matching marker pair")
    begin, end = markers
    indent = re.match(r"[ \t]*", begin[0])[0]
    body = "\n".join(indent + line if line else "" for line in generated.splitlines())
    return source[:begin.end()] + "\n" + body + "\n" + source[end.start():]


def formatted(source: str) -> str:
    result = subprocess.run(
        ["rustfmt", "--edition", "2024", "--emit", "stdout"], input=source,
        text=True, encoding="utf-8", capture_output=True, check=True,
    )
    return result.stdout


def generate(root: Path, check: bool) -> list[Path]:
    catalog = load_catalog(root / CATALOG)
    candidates = []
    # Prepare every replacement before touching a source file.
    for region, relative in TARGETS.items():
        path = root / relative
        original = path.read_bytes()
        newline = "\r\n" if b"\r\n" in original else "\n"
        source = original.decode("utf-8").replace("\r\n", "\n")
        expected = formatted(replace_region(source, region, render(catalog, region)))
        encoded = expected.replace("\r\n", "\n").replace("\n", newline).encode("utf-8")
        if encoded != original:
            candidates.append((relative, encoded))
    if not check:
        for relative, encoded in candidates:
            (root / relative).write_bytes(encoded)
    return [relative for relative, _ in candidates]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Report drift without modifying source")
    args = parser.parse_args()
    try:
        changed = generate(ROOT, args.check)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"Scalar generation failed: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError):
            print(error.stderr, file=sys.stderr)
        return 2
    for path in changed:
        print(f"{'Out of date' if args.check else 'Updated'}: {path.as_posix()}")
    if not changed:
        print("Scalar instruction regions are up to date")
    return 1 if args.check and changed else 0


if __name__ == "__main__":
    raise SystemExit(main())

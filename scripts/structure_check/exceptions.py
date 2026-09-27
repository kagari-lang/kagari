"""Apply documented, bounded structural exceptions without hiding their findings."""

from pathlib import Path, PurePosixPath
import re
import tomllib

from structure_check.rust import LOC_LIMIT

POLICY_PATH = "scripts/structure-exceptions.toml"
COMMON_FIELDS = {"path", "rule", "reason", "evidence"}


def normalize_declaration(text: str) -> str:
    return re.sub(r"\s+", "", text)


def repository_file(root: Path, relative: str) -> Path:
    path = PurePosixPath(relative)
    if path.is_absolute() or ".." in path.parts or "\\" in relative or ":" in relative:
        raise ValueError(f"exception path must be repository-relative: {relative}")
    resolved = (root / relative).resolve()
    if not resolved.is_relative_to(root.resolve()) or not resolved.is_file():
        raise ValueError(f"exception refers to a missing or external file: {relative}")
    return resolved


def apply_exceptions(report: dict, root: Path) -> dict:
    policy = root / POLICY_PATH
    config = tomllib.loads(policy.read_text(encoding="utf-8")) if policy.exists() else {}
    if set(config) - {"exceptions"}:
        raise ValueError(f"unknown keys in {POLICY_PATH}")
    entries = config.get("exceptions", [])
    if not isinstance(entries, list):
        raise ValueError("exceptions must be an array of tables")
    remaining = list(report["findings"])
    accepted = []
    seen = set()
    for entry in entries:
        if not isinstance(entry, dict):
            raise ValueError("each exception must be a table")
        for field in COMMON_FIELDS:
            if not isinstance(entry.get(field), str) or not entry[field].strip():
                raise ValueError(f"exception requires a nonempty {field}")
        rule, path = entry["rule"], entry["path"]
        repository_file(root, path)
        # Evidence is a reviewable, nonempty repository document, not a bare waiver.
        evidence = repository_file(root, entry["evidence"])
        if evidence.suffix != ".md" or not evidence.read_text(encoding="utf-8").strip():
            raise ValueError("exception evidence must be a nonempty Markdown document")
        if rule == "effective-loc":
            extra = {"max_loc"}
            ceiling = entry.get("max_loc")
            if type(ceiling) is not int or ceiling <= LOC_LIMIT:
                raise ValueError(f"effective-loc exception requires max_loc greater than {LOC_LIMIT}")
            subject = ""
        elif rule == "reexport-location":
            extra = {"declaration"}
            declaration = entry.get("declaration")
            if not isinstance(declaration, str) or not declaration.strip():
                raise ValueError("reexport exception requires an exact declaration")
            subject = normalize_declaration(declaration)
        else:
            raise ValueError(f"rule does not support exceptions: {rule}")
        if set(entry) - COMMON_FIELDS - extra:
            raise ValueError(f"unknown fields in exception for {path}: {rule}")
        key = path, rule, subject
        if key in seen:
            raise ValueError(f"duplicate exception: {path}: {rule}")
        seen.add(key)
        matches = [finding for finding in remaining
                   if finding["path"] == path and finding["rule"] == rule
                   and (rule == "effective-loc" or normalize_declaration(
                       finding.get("declaration", "")) == subject)]
        if len(matches) != 1:
            raise ValueError(f"stale or ambiguous exception: {path}: {rule}; "
                             "expected exactly one matching finding")
        finding = matches[0]
        if rule == "effective-loc" and finding["effective_loc"] > ceiling:
            # Keep growth beyond the reviewed ceiling as an ordinary failure.
            finding = dict(finding, message=finding["message"]
                           + f" (documented exception ceiling: {ceiling})")
            remaining[remaining.index(matches[0])] = finding
            continue
        remaining.remove(finding)
        accepted.append(dict(finding, reason=entry["reason"], evidence=entry["evidence"],
                             **({"max_loc": ceiling} if rule == "effective-loc" else {})))
    return dict(report, findings=remaining, exceptions=accepted)

"""Syntax-aware import, path, re-export and effective-LOC checks."""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
import posixpath
import re
import tomllib

from tree_sitter import Language, Node, Parser, Tree
import tree_sitter_rust

LANGUAGE = Language(tree_sitter_rust.language())
COMMENTS = {"line_comment", "block_comment"}
PATHS = {"scoped_identifier", "scoped_type_identifier"}
LITERALS = {"string_literal", "raw_string_literal", "char_literal"}
OPAQUE = {"macro_definition", "macro_invocation"}
LOC_LIMIT = 1200


def descendants(node: Node):
    pending = [node]
    while pending:
        current = pending.pop()
        yield current
        pending.extend(reversed(current.children))


def effective_loc(data: bytes, root: Node) -> int:
    """Remove comment bytes, preserving strings, line endings and mixed lines."""
    masked = bytearray(data)
    for node in descendants(root):
        if node.type in COMMENTS:
            for offset in range(node.start_byte, node.end_byte):
                if masked[offset] not in (10, 13):
                    masked[offset] = 32
    return sum(bool(line.strip()) for line in masked.splitlines())


def cfg_possibilities(expression: str) -> set[bool]:
    """Conservatively evaluate cfg with test=false and all other flags unknown."""
    tokens = re.findall(r'"(?:[^"\\]|\\.)*"|[A-Za-z_][A-Za-z_0-9]*|[(),=]', expression)
    index = 0

    def read() -> set[bool]:
        nonlocal index
        if index >= len(tokens):
            return {False, True}
        name = tokens[index]
        index += 1
        if index < len(tokens) and tokens[index] == "=":
            index += 2
            return {False, True}
        if index < len(tokens) and tokens[index] == "(":
            index += 1
            values = []
            while index < len(tokens) and tokens[index] != ")":
                values.append(read())
                if index < len(tokens) and tokens[index] == ",":
                    index += 1
            if index >= len(tokens):
                return {False, True}
            index += 1
            if name == "not" and len(values) == 1:
                return {not value for value in values[0]}
            if name in {"all", "any"}:
                result = {name == "all"}
                for value in values:
                    result = {a and b if name == "all" else a or b
                              for a in result for b in value}
                return result
            return {False, True}
        return {False} if name == "test" else {False, True}

    result = read()
    return result if index == len(tokens) else {False, True}


def cfg_only_test(attributes: list[Node]) -> bool:
    for attribute in attributes:
        # cfg_attr does not itself make the following item test-only.
        text = without_comments(attribute)
        match = re.fullmatch(r"#\s*!?\s*\[\s*cfg\s*\((.*)\)\s*\]", text, re.S)
        if match and True not in cfg_possibilities(match[1]):
            return True
    return False


def without_comments(node: Node) -> str:
    data = bytearray(node.text)
    for part in descendants(node):
        if part.type in COMMENTS:
            start, end = part.start_byte - node.start_byte, part.end_byte - node.start_byte
            data[start:end] = b" " * (end - start)
    return data.decode("utf-8")


def path_parts(node: Node) -> list[str]:
    """Only inspect the path spine, not generic arguments or qualified traits."""
    if node.type in PATHS:
        base = node.child_by_field_name("path")
        name = node.child_by_field_name("name")
        return (path_parts(base) if base else []) + ([name.text.decode()] if name else [])
    if node.type in {"generic_type", "generic_function"}:
        base = node.child_by_field_name("type") or node.child_by_field_name("function")
        return path_parts(base) if base else []
    if node.type in {"identifier", "type_identifier", "crate", "self", "super"}:
        return [node.text.decode()]
    return []


def import_paths(node: Node | None, prefix: list[str] | None = None):
    """Expand grouped use trees for parent traversal checks."""
    prefix = prefix or []
    if node is None:
        return
    if node.type == "scoped_use_list":
        base = node.child_by_field_name("path")
        yield from import_paths(node.child_by_field_name("list"),
                                prefix + (path_parts(base) if base else []))
    elif node.type == "use_list":
        for child in node.named_children:
            yield from import_paths(child, prefix)
    elif node.type == "use_as_clause":
        yield from import_paths(node.child_by_field_name("path"), prefix)
    elif node.type == "use_wildcard":
        base = next((c for c in node.named_children if c.type not in COMMENTS), None)
        yield prefix + (path_parts(base) if base else [])
    else:
        yield prefix + path_parts(node)


@dataclass
class Source:
    path: str
    data: bytes
    tree: Tree
    dependencies: set[str]

    def diagnostic(self, rule: str, node: Node, message: str) -> dict:
        return {"path": self.path, "line": node.start_point.row + 1,
                "column": node.start_point.column + 1, "rule": rule, "message": message}


def items(node: Node, test_only: bool):
    inner = [c for c in node.named_children if c.type == "inner_attribute_item"]
    test_only = test_only or cfg_only_test(inner)
    attributes = []
    for child in node.named_children:
        if child.type in COMMENTS or child.type == "inner_attribute_item":
            continue
        if child.type == "attribute_item":
            attributes.append(child)
            continue
        yield child, test_only or cfg_only_test(attributes), attributes
        attributes = []


def module_edges(source: Source, available: set[str], entrypoint=False) -> list[tuple[str, bool]]:
    edges = []
    parent = posixpath.dirname(source.path) or "."
    stem = posixpath.basename(source.path).removesuffix(".rs")
    directory = parent if entrypoint or stem in {"lib", "main", "mod"} else f"{parent}/{stem}"

    def visit(node: Node, test_only: bool, directory: str, attribute_base: str):
        for child, child_test, attributes in items(node, test_only):
            if child.type in OPAQUE or child.type in LITERALS:
                continue
            if child.type == "mod_item":
                name_node = child.child_by_field_name("name")
                if name_node is None or name_node.is_missing:
                    continue
                name = name_node.text.decode()
                body = child.child_by_field_name("body")
                if body:
                    visit(body, child_test, f"{directory}/{name}", f"{directory}/{name}")
                    continue
                candidates = [f"{directory}/{name}.rs", f"{directory}/{name}/mod.rs"]
                for attribute in attributes:
                    match = re.fullmatch(r'#\[\s*path\s*=\s*"([^"\\]+)"\s*\]',
                                         without_comments(attribute))
                    if match:
                        candidates = [f"{attribute_base}/{match[1]}"]
                for candidate in candidates:
                    candidate = posixpath.normpath(candidate)
                    if candidate in available:
                        edges.append((candidate, child_test))
            else:
                visit(child, child_test, directory, attribute_base)

    visit(source.tree.root_node, False, directory, parent)
    return edges


def manifest_context(manifests: dict[str, bytes], paths: set[str]):
    roots: dict[str, set[bool]] = {}
    dependencies: dict[str, set[str]] = {}
    # A nested package owns its source even when a parent package is also present.
    for manifest, content in sorted(manifests.items(), key=lambda item: len(item[0])):
        config = tomllib.loads(content.decode("utf-8"))
        if "package" not in config:
            continue
        parent = posixpath.dirname(manifest)
        prefix = parent + "/" if parent else ""
        names = {"std", "core", "alloc", "crate"}
        for category in ("dependencies", "build-dependencies", "dev-dependencies"):
            names.update(name.replace("-", "_") for name in config.get(category, {}))
            for target in config.get("target", {}).values():
                names.update(name.replace("-", "_") for name in target.get(category, {}))
        for path in paths:
            if path.startswith(prefix):
                dependencies[path] = names
                relative = path[len(prefix):]
                for folder, auto, test_only in [("tests/", "autotests", True),
                                               ("examples/", "autoexamples", False),
                                               ("benches/", "autobenches", False),
                                               ("src/bin/", "autobins", False)]:
                    if relative.startswith(folder) and config["package"].get(auto, True):
                        target = relative[len(folder):]
                        if "/" not in target or (target.count("/") == 1 and target.endswith("/main.rs")):
                            roots.setdefault(path, set()).add(test_only)
        library = config.get("lib", {}).get("path", "src/lib.rs")
        targets = [(library, False), ("src/main.rs", False), ("build.rs", False)]
        for kind in ("bin", "test", "bench", "example"):
            targets.extend((target["path"], kind == "test")
                           for target in config.get(kind, []) if "path" in target)
        for target, test_only in targets:
            path = posixpath.normpath(prefix + target)
            if path in paths:
                roots.setdefault(path, set()).add(test_only)
    return roots, dependencies


def test_files(sources: dict[str, Source], roots: dict[str, set[bool]]) -> set[str]:
    available = set(sources)
    edges = {path: module_edges(source, available, path in roots)
             for path, source in sources.items()}
    states: dict[str, set[bool]] = {p: set() for p in sources}

    def propagate(seeds):
        queue = deque(seeds)
        while queue:
            path, test_only = queue.popleft()
            if test_only in states[path]:
                continue
            states[path].add(test_only)
            for target, local_test in edges[path]:
                queue.append((target, test_only or local_test))

    propagate((path, flag) for path, flags in roots.items() for flag in flags)
    incoming = {target for values in edges.values() for target, _ in values}
    propagate((path, False) for path in sources if not states[path] and path not in incoming)
    # Unreachable cycles/unknown entrypoints receive production checks.
    for path in sources:
        if not states[path]:
            propagate([(path, False)])
    return {path for path, flags in states.items() if flags == {True}}


def inspect(source: Source, test_only: bool) -> list[dict]:
    findings = []

    def emit(rule, node, message):
        finding = source.diagnostic(rule, node, message)
        findings.append(finding)
        return finding

    def visit(node: Node, test_only: bool, inside_use=False, inside_path=False, ufcs=False):
        if node.type in COMMENTS or node.type in LITERALS:
            return
        if node.type in OPAQUE:
            # Macro bodies are token trees, not parsed Rust items/expressions.
            return
        if node.type in {"attribute_item", "inner_attribute_item"}:
            return
        if node.type == "use_declaration":
            visibility = next((c for c in node.named_children
                               if c.type == "visibility_modifier"), None)
            if visibility:
                finding = emit("reexport-whitelist", node,
                               "import from the owner; intentional re-exports require an exact whitelist entry")
                finding["declaration"] = without_comments(node).strip()
            if not test_only:
                if any(n.type == "use_wildcard" for n in descendants(node)):
                    emit("wildcard-import", node, "list production imports and re-exports explicitly")
                paths = import_paths(node.child_by_field_name("argument"))
                if any(any(parts[i:i + 2] == ["super", "super"]
                           for i in range(len(parts) - 1)) for parts in paths):
                    emit("parent-traversal", node, "replace repeated super:: with a crate-root import")
            inside_use = True
        if node.type in PATHS and not inside_use and not test_only:
            parts = path_parts(node)
            is_outer = not inside_path
            if is_outer and any(parts[i:i + 2] == ["super", "super"]
                                for i in range(len(parts) - 1)):
                emit("parent-traversal", node, "replace repeated super:: with a crate-root import")
            elif is_outer and not ufcs and (
                len(parts) >= 3 or (len(parts) >= 2 and parts[0] in source.dependencies)
                or node.text.lstrip().startswith(b"::")
            ):
                emit("qualified-path", node, "import the item or use a short module qualifier: "
                     + "::".join(parts))
        for child, child_test, _ in items(node, test_only):
            # Type arguments contain independent paths that need their own check.
            path_child = node.type in PATHS and child == node.child_by_field_name("path")
            generic_spine = node.type in {"generic_type", "generic_function"} and (
                child == node.child_by_field_name("type")
                or child == node.child_by_field_name("function"))
            visit(child, child_test, inside_use,
                  path_child or (inside_path and generic_spine),
                  ufcs or node.type in {"qualified_type", "bracketed_type"})

    root = source.tree.root_node
    for node in descendants(root):
        if node.is_error or node.is_missing:
            emit("parse-error", node, "Rust syntax could not be checked; fix or investigate the syntax")
    visit(root, test_only)
    loc = effective_loc(source.data, root)
    if loc > LOC_LIMIT:
        finding = emit("effective-loc", root,
                       f"{loc} effective LOC exceeds {LOC_LIMIT}; split by responsibility")
        finding["effective_loc"] = loc
    return findings


def check_sources(raw: dict[str, bytes], manifests: dict[str, bytes] | None = None) -> dict:
    parser = Parser(LANGUAGE)
    roots, dependencies = manifest_context(manifests or {}, set(raw))
    sources = {path: Source(path, data, parser.parse(data),
                           dependencies.get(path, {"std", "core", "alloc", "crate"}))
               for path, data in raw.items()}
    tests = test_files(sources, roots)
    findings = []
    for path, source in sources.items():
        findings.extend(inspect(source, path in tests))
    findings.sort(key=lambda f: (f["path"], f["line"], f["column"], f["rule"]))
    return {"files": len(sources), "findings": findings}

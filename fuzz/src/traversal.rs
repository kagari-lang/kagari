//! Cross-check public traversal contracts without another parser or stored tree.

use kagari_common::span::Span;
use kagari_syntax::{
    ast::{
        module::ModuleSyntax,
        visit::{ElementRef, NodeRef},
    },
    kind::SyntaxKind,
};

type NodeRecord = (SyntaxKind, Span);

fn record(node: NodeRef<'_>) -> NodeRecord {
    (node.kind(), node.body_span())
}

// A deterministic source-derived selection exercises pruning on malformed trees
// too. The root remains traversable so sibling continuation can be observed.
fn descend(node: NodeRef<'_>, root: bool) -> bool {
    root || !(node.body_span().start + node.body_span().end).is_multiple_of(3)
}

fn expected(root: NodeRef<'_>, owned: bool, prune: bool) -> Vec<NodeRecord> {
    let mut result = Vec::new();
    let mut pending = vec![(root, true)];
    while let Some((node, is_root)) = pending.pop() {
        if owned && !is_root && matches!(node, NodeRef::ModuleBody(_)) {
            continue;
        }
        result.push(record(node));
        if !prune || descend(node, is_root) {
            for element in node.elements().into_iter().rev() {
                if let ElementRef::Node(child) = element {
                    pending.push((child, false));
                }
            }
        }
    }
    result
}

pub(crate) fn check_physical(root: NodeRef<'_>) {
    let all = expected(root, false, false);
    assert_eq!(root.descendants().map(record).collect::<Vec<_>>(), all);
    for prune in [false, true] {
        let mut visited = Vec::new();
        root.visit(&mut |node| {
            let is_root = visited.is_empty();
            visited.push(record(node));
            !prune || descend(node, is_root)
        });
        if prune {
            assert_eq!(visited, expected(root, false, true));
        } else {
            assert_eq!(visited, all);
        }
    }
}

pub(crate) fn check_owned(module: &ModuleSyntax) {
    let root = NodeRef::ModuleBody(module.body());
    for prune in [false, true] {
        let mut visited = Vec::new();
        module.visit_owned(&mut |node| {
            let is_root = visited.is_empty();
            visited.push(record(node));
            !prune || descend(node, is_root)
        });
        assert_eq!(visited, expected(root, true, prune));
    }
}

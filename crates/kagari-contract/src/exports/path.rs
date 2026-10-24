//! Bounded public-path traversal over verified export namespaces.

use crate::exports::{ExportNamespace, ProgramExports, TypeExport};
use kagari_common::identity::{MAX_IDENTITY_PATH_SEGMENTS, ModuleIdentity, table::DefinitionId};

/// The namespace requested at the final segment, independent of other names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicPathKind {
    Module,
    Function,
    Type,
}

/// Canonical checked graph evidence, not an executable slot or a source spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicPathTarget<'a> {
    Module(&'a ModuleIdentity),
    Function(DefinitionId),
    Type(&'a TypeExport),
}

/// Public lookup failures keep absent and private names indistinguishable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicPathError {
    InvalidPath,
    NotFound,
    WrongTargetKind,
    /// A supplied namespace or module edge has no verified namespace record.
    InvalidGraph,
}

impl ProgramExports {
    /// Resolve 1..64 identifier segments relative to a canonical namespace.
    ///
    /// For `api::users`, module aliases select their canonical target, including
    /// logical prefix namespaces with no executable module. Cycles require no
    /// expansion: traversal takes one edge per segment. The requested leaf kind
    /// wins over an identically named item in a different namespace.
    ///
    /// Syntax is checked in full before any lookup. Identifiers follow Kagari's
    /// ASCII source grammar; `crate`, `self` and `super` navigation is forbidden.
    /// Private/missing intermediates return `NotFound`; a public leaf only in a
    /// different namespace returns `WrongTargetKind`. An empty export graph has
    /// no public paths. Missing records in a nonempty verified graph instead
    /// return `InvalidGraph`, which the runtime must treat as an engine fault.
    pub fn resolve_public_path(
        &self,
        start: &ModuleIdentity,
        path: &str,
        kind: PublicPathKind,
    ) -> Result<PublicPathTarget<'_>, PublicPathError> {
        validate_path(path)?;
        if self.namespaces.is_empty() {
            return Err(PublicPathError::NotFound);
        }
        let mut namespace = self
            .namespaces
            .get(start)
            .ok_or(PublicPathError::InvalidGraph)?;
        let mut parts = path.split("::").peekable();
        while let Some(name) = parts.next() {
            if parts.peek().is_none() {
                let target = leaf(namespace, name, kind)?;
                if let PublicPathTarget::Module(identity) = target
                    && !self.namespaces.contains_key(identity)
                {
                    return Err(PublicPathError::InvalidGraph);
                }
                return Ok(target);
            }
            let next = namespace
                .modules
                .get(name)
                .ok_or(PublicPathError::NotFound)?;
            namespace = self
                .namespaces
                .get(next)
                .ok_or(PublicPathError::InvalidGraph)?;
        }
        Err(PublicPathError::InvalidPath)
    }
}

fn validate_path(path: &str) -> Result<(), PublicPathError> {
    for (index, name) in path.split("::").enumerate() {
        let mut bytes = name.bytes();
        if index >= MAX_IDENTITY_PATH_SEGMENTS
            || matches!(name, "crate" | "self" | "super")
            || !bytes
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == b'_')
            || !bytes.all(|ch| ch.is_ascii_alphanumeric() || ch == b'_')
        {
            return Err(PublicPathError::InvalidPath);
        }
    }
    Ok(())
}

fn leaf<'a>(
    namespace: &'a ExportNamespace,
    name: &str,
    kind: PublicPathKind,
) -> Result<PublicPathTarget<'a>, PublicPathError> {
    let target = match kind {
        PublicPathKind::Module => namespace.modules.get(name).map(PublicPathTarget::Module),
        PublicPathKind::Function => namespace
            .functions
            .get(name)
            .copied()
            .map(PublicPathTarget::Function),
        PublicPathKind::Type => namespace.types.get(name).map(PublicPathTarget::Type),
    };
    target.ok_or_else(|| {
        if namespace.modules.contains_key(name)
            || namespace.functions.contains_key(name)
            || namespace.types.contains_key(name)
        {
            PublicPathError::WrongTargetKind
        } else {
            PublicPathError::NotFound
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
    use kagari_types::{scalar::BuiltinType, ty::Ty};

    fn graph() -> (ProgramExports, ModuleIdentity, ModuleIdentity) {
        let root = ModuleIdentity::single_file("game");
        let users = ModuleIdentity {
            package: root.package.clone(),
            path: vec!["internal".into(), "users".into()],
        };
        let graph = ProgramExports {
            namespaces: [
                (
                    root.clone(),
                    ExportNamespace {
                        modules: [
                            ("api".into(), users.clone()),
                            ("users".into(), users.clone()),
                        ]
                        .into(),
                        types: [(
                            "Count".into(),
                            TypeExport::Closed(Ty::Builtin(BuiltinType::I32)),
                        )]
                        .into(),
                        ..Default::default()
                    },
                ),
                (
                    users.clone(),
                    ExportNamespace {
                        modules: [
                            ("again".into(), users.clone()),
                            ("root".into(), root.clone()),
                        ]
                        .into(),
                        ..Default::default()
                    },
                ),
            ]
            .into(),
            entry: None,
        };
        (graph, root, users)
    }

    #[test]
    fn module_aliases_cycles_and_relative_namespaces_are_canonical() {
        let (graph, root, users) = graph();
        for path in ["api", "users", "api::again", "api::root::users"] {
            assert_eq!(
                graph.resolve_public_path(&root, path, PublicPathKind::Module),
                Ok(PublicPathTarget::Module(&users))
            );
        }
        assert_eq!(
            graph.resolve_public_path(&users, "root", PublicPathKind::Module),
            Ok(PublicPathTarget::Module(&root))
        );
        assert_eq!(
            graph.resolve_public_path(&users, "users", PublicPathKind::Module),
            Err(PublicPathError::NotFound)
        );
        assert_eq!(
            graph.resolve_public_path(&root, "internal", PublicPathKind::Module),
            Err(PublicPathError::NotFound)
        );
    }

    #[test]
    fn whole_path_syntax_precedes_missing_names_and_bounds_cycles() {
        let (graph, root, _) = graph();
        for path in [
            "",
            "::api",
            "api::",
            "api::::again",
            "absent::bad-name",
            "absent::self",
            "api<T>",
            "api.again",
            "0api",
            " api",
            "café",
            "super",
            "crate",
        ] {
            assert_eq!(
                graph.resolve_public_path(&root, path, PublicPathKind::Module),
                Err(PublicPathError::InvalidPath),
                "{path}"
            );
        }
        let path = std::iter::once("api")
            .chain(std::iter::repeat_n("again", MAX_IDENTITY_PATH_SEGMENTS - 1))
            .collect::<Vec<_>>()
            .join("::");
        assert!(
            graph
                .resolve_public_path(&root, &path, PublicPathKind::Module)
                .is_ok()
        );
        assert_eq!(
            graph.resolve_public_path(&root, &format!("{path}::again"), PublicPathKind::Module),
            Err(PublicPathError::InvalidPath)
        );
    }

    #[test]
    fn requested_namespace_and_graph_failures_are_distinct() {
        let (mut graph, root, users) = graph();
        let definitions = DefinitionContext::new().unwrap();
        let function = definitions
            .intern_child(
                definitions.intern_root(&root).unwrap(),
                DefinitionKind::Function,
                "api",
                0,
            )
            .unwrap();
        graph
            .namespaces
            .get_mut(&root)
            .unwrap()
            .functions
            .extend([("api".into(), function), ("Count".into(), function)]);
        assert_eq!(
            graph.resolve_public_path(&root, "api", PublicPathKind::Module),
            Ok(PublicPathTarget::Module(&users))
        );
        assert_eq!(
            graph.resolve_public_path(&root, "api", PublicPathKind::Function),
            Ok(PublicPathTarget::Function(function))
        );
        assert_eq!(
            graph.resolve_public_path(&root, "Count", PublicPathKind::Type),
            Ok(PublicPathTarget::Type(&TypeExport::Closed(Ty::Builtin(
                BuiltinType::I32
            ))))
        );
        assert_eq!(
            graph.resolve_public_path(&root, "Count", PublicPathKind::Module),
            Err(PublicPathError::WrongTargetKind)
        );
        assert_eq!(
            graph.resolve_public_path(&root, "Count::child", PublicPathKind::Module),
            Err(PublicPathError::NotFound)
        );
        assert_eq!(
            graph.resolve_public_path(&root, "absent", PublicPathKind::Module),
            Err(PublicPathError::NotFound)
        );
        graph.namespaces.remove(&users);
        assert_eq!(
            graph.resolve_public_path(&root, "api", PublicPathKind::Module),
            Err(PublicPathError::InvalidGraph)
        );
        assert_eq!(
            ProgramExports::default().resolve_public_path(&root, "api", PublicPathKind::Module),
            Err(PublicPathError::NotFound)
        );
    }
}

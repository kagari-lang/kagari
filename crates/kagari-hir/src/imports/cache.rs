//! Compare name inputs before the existing arena-aware signature/body remappers.
use crate::imports::{
    BindingCandidate, BindingOrigin, DirectiveResolution, ModuleImportFacts, NamespaceId,
    ResolvedTarget, SourceDeclRef, SourceUnit,
};
impl ModuleImportFacts {
    pub(crate) fn same_bindings(&self, other: &Self) -> bool {
        self.same_surface(other, false)
    }
    pub(crate) fn same_signature_bindings(&self, other: &Self) -> bool {
        self.same_surface(other, true)
    }
    fn same_surface(&self, other: &Self, signature: bool) -> bool {
        let local = self.scope.unit.as_ref();
        let old_local = other.scope.unit.as_ref();
        let target_eq = |a: &ResolvedTarget, b: &ResolvedTarget| {
            target_equal(a, b, local, old_local, signature)
        };
        self.array_interfaces == other.array_interfaces
            && self.diagnostics.is_empty()
            && other.diagnostics.is_empty()
            && self.dependencies == other.dependencies
            && self.directives.len() == other.directives.len()
            && self.directives.iter().zip(&other.directives).all(|(a, b)| {
                a.path == b.path
                    && a.kind == b.kind
                    && a.visibility == b.visibility
                    && a.direct_dependencies == b.direct_dependencies
                    && match (&a.resolution, &b.resolution) {
                        (DirectiveResolution::Resolved(a), DirectiveResolution::Resolved(b)) => {
                            target_eq(a, b)
                        }
                        (a, b) => a == b,
                    }
            })
            && self.scope.impl_count() == other.scope.impl_count()
            && self.scope.entries.len() == other.scope.entries.len()
            && self.scope.entries.iter().all(|(name, a)| {
                other.scope.entries.get(name).is_some_and(|b| {
                    let eq = |a: &[BindingCandidate], b: &[BindingCandidate]| {
                        a.len() == b.len()
                            && a.iter().zip(b).all(|(a, b)| {
                                a.owner == b.owner
                                    && a.visibility == b.visibility
                                    && origin_equal(&a.origin, &b.origin)
                                    && match (&a.target, &b.target) {
                                        (Some(a), Some(b)) => target_eq(a, b),
                                        (None, None) => true,
                                        _ => false,
                                    }
                            })
                    };
                    eq(&a.strong, &b.strong)
                        && eq(&a.globs, &b.globs)
                        && eq(&a.implicit, &b.implicit)
                })
            })
    }
}
fn unit_equal(
    a: &SourceUnit,
    b: &SourceUnit,
    local: Option<&SourceUnit>,
    old_local: Option<&SourceUnit>,
    signature: bool,
) -> bool {
    a == b
        || ((signature || (Some(a) == local && Some(b) == old_local))
            && a.module == b.module
            && a.file == b.file)
}
fn source_equal(
    a: &SourceDeclRef,
    b: &SourceDeclRef,
    local: Option<&SourceUnit>,
    old_local: Option<&SourceUnit>,
    signature: bool,
) -> bool {
    a.item == b.item && unit_equal(&a.unit, &b.unit, local, old_local, signature)
}
fn target_equal(
    a: &ResolvedTarget,
    b: &ResolvedTarget,
    local: Option<&SourceUnit>,
    old_local: Option<&SourceUnit>,
    signature: bool,
) -> bool {
    match (a, b) {
        (ResolvedTarget::Source(a), ResolvedTarget::Source(b)) => {
            source_equal(a, b, local, old_local, signature)
        }
        (
            ResolvedTarget::Namespace(NamespaceId::Module(a)),
            ResolvedTarget::Namespace(NamespaceId::Module(b)),
        ) => unit_equal(a, b, local, old_local, signature),
        (
            ResolvedTarget::Namespace(NamespaceId::Associated(a)),
            ResolvedTarget::Namespace(NamespaceId::Associated(b)),
        ) => source_equal(a, b, local, old_local, signature),
        _ => a == b,
    }
}
fn origin_equal(a: &BindingOrigin, b: &BindingOrigin) -> bool {
    match (a, b) {
        (BindingOrigin::Declaration(a), BindingOrigin::Declaration(b)) => {
            a.item == b.item && a.unit.module == b.unit.module && a.unit.file == b.unit.file
        }
        (
            BindingOrigin::ModuleDeclaration {
                unit: a,
                module: am,
            },
            BindingOrigin::ModuleDeclaration {
                unit: b,
                module: bm,
            },
        ) => a.module == b.module && a.file == b.file && am == bm,
        (BindingOrigin::NamedImport(a), BindingOrigin::NamedImport(b))
        | (BindingOrigin::GlobImport(a), BindingOrigin::GlobImport(b)) => {
            a.unit.module == b.unit.module && a.unit.file == b.unit.file && a.slot == b.slot
        }
        _ => a == b,
    }
}

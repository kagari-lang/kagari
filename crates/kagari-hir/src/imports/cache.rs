//! Compare name inputs before the existing arena-aware signature/body remappers.
use crate::{
    imports::{
        BindingCandidate, BindingOrigin, DirectiveResolution, ModuleImportFacts, NamespaceId,
        ResolvedTarget, SourceDeclRef, SourceItem, SourceUnit,
    },
    resolver::table::NameTable,
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
            && same_name_tables(&self.scope, &other.scope, local, old_local, signature)
    }
}

pub(crate) fn same_name_tables(
    a: &NameTable,
    b: &NameTable,
    local: Option<&SourceUnit>,
    other_local: Option<&SourceUnit>,
    signature: bool,
) -> bool {
    a.impl_count() == b.impl_count()
        && a.entries.len() == b.entries.len()
        && a.entries.iter().all(|(name, a)| {
            b.entries.get(name).is_some_and(|b| {
                let eq = |a: &[BindingCandidate], b: &[BindingCandidate]| {
                    a.len() == b.len()
                        && a.iter().zip(b).all(|(a, b)| {
                            a.owner == b.owner
                                && a.visibility == b.visibility
                                && origin_equal(&a.origin, &b.origin, local, other_local)
                                && match (&a.target, &b.target) {
                                    (Some(a), Some(b)) => {
                                        target_equal(a, b, local, other_local, signature)
                                    }
                                    (None, None) => true,
                                    _ => false,
                                }
                        })
                };
                eq(&a.strong, &b.strong) && eq(&a.globs, &b.globs) && eq(&a.implicit, &b.implicit)
            })
        })
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
    same_source_item(a, b, local, old_local)
        && unit_equal(&a.unit, &b.unit, local, old_local, signature)
}

fn same_source_item(
    a: &SourceDeclRef,
    b: &SourceDeclRef,
    local: Option<&SourceUnit>,
    other_local: Option<&SourceUnit>,
) -> bool {
    if let (SourceItem::Variant(a_id), SourceItem::Variant(b_id)) = (a.item, b.item)
        && Some(&a.unit) == local
        && Some(&b.unit) == other_local
    {
        // Variant IDs embed an arena, unlike the declaration item IDs. Rebase
        // only well-formed members of the two local units; the remapper still
        // validates the declaration surface before restoring checked facts.
        return a_id.arena() == a.unit.arena
            && b_id.arena() == b.unit.arena
            && a_id.owner() == b_id.owner()
            && a_id.slot() == b_id.slot();
    }
    a.item == b.item
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
fn origin_equal(
    a: &BindingOrigin,
    b: &BindingOrigin,
    local: Option<&SourceUnit>,
    other_local: Option<&SourceUnit>,
) -> bool {
    match (a, b) {
        (BindingOrigin::Declaration(a), BindingOrigin::Declaration(b)) => {
            same_source_item(a, b, local, other_local)
                && a.unit.module == b.unit.module
                && a.unit.file == b.unit.file
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

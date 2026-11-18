//! Demand-built declaration-site selection in analysis coordinates.

use kagari_common::span::Span;
use kagari_hir::{
    analysis::declarations::{DeclarationSnapshot, FileDeclarations},
    declarations::{Declaration, Declarations},
    hir::{expr::ExprKind, ids::GenericParamId, pattern::PatternKind},
    lower::LoweredModule,
    resolver::resolved::ResolvedName,
};

#[derive(Debug)]
struct DeclarationSite<'a> {
    range: Span,
    declaration: &'a Declaration,
    member: bool,
}

/// Analysis-coordinate sites borrowing canonical declarations, never physical-origin copies.
#[derive(Debug, Default)]
struct DeclarationSiteIndex<'a> {
    sites: Vec<DeclarationSite<'a>>,
}

impl<'a> DeclarationSiteIndex<'a> {
    fn new(lowered: &LoweredModule, declarations: &'a Declarations) -> Self {
        let mut index = Self::default();
        if lowered.source_surface().is_none() {
            return index;
        }
        let module = &lowered.module;
        let mut named = |span, name| {
            if let Some(span) = span {
                index.insert(span, declarations.target(name), false);
            }
        };
        for item in &module.functions {
            named(item.name_span, ResolvedName::Function(item.id));
        }
        for item in &module.consts {
            named(item.name_span, ResolvedName::Const(item.id));
        }
        for item in &module.statics {
            named(item.name_span, ResolvedName::Static(item.id));
        }
        for item in &module.modules {
            named(item.name_span, ResolvedName::Module(item.id));
        }
        for item in &module.type_aliases {
            named(item.name_span, ResolvedName::TypeAlias(item.id));
        }
        for item in &module.native_types {
            named(item.name_span, ResolvedName::NativeType(item.id));
        }
        for item in &module.structs {
            named(item.name_span, ResolvedName::Struct(item.id));
        }
        for item in &module.enums {
            named(item.name_span, ResolvedName::Enum(item.id));
        }
        for item in &module.traits {
            named(item.name_span, ResolvedName::Trait(item.id));
        }
        for item in &module.functions {
            index.generics(lowered, declarations, &item.generic_params);
            for param in &item.params {
                index.insert(
                    param.binding_span(),
                    declarations.target(ResolvedName::Param(param.id)),
                    false,
                );
            }
        }
        for item in &module.type_aliases {
            index.generics(lowered, declarations, &item.generic_params);
        }
        for item in &module.native_types {
            index.generics(lowered, declarations, &item.generic_params);
            for field in &item.fields {
                index.insert(
                    field.diagnostic_name_span(),
                    declarations.field(field.id),
                    true,
                );
            }
        }
        for item in &module.structs {
            index.generics(lowered, declarations, &item.generic_params);
            for field in &item.fields {
                index.insert(
                    field.diagnostic_name_span(),
                    declarations.field(field.id),
                    true,
                );
            }
        }
        for item in &module.enums {
            index.generics(lowered, declarations, &item.generic_params);
            for variant in &item.variants {
                index.insert(
                    variant.diagnostic_name_span(),
                    declarations.variant(variant.id),
                    true,
                );
            }
        }
        for (generics, constants, types) in module
            .traits
            .iter()
            .map(|item| {
                (
                    &item.generic_params,
                    &item.associated_consts,
                    &item.associated_types,
                )
            })
            .chain(module.impls.iter().map(|item| {
                (
                    &item.generic_params,
                    &item.associated_consts,
                    &item.associated_types,
                )
            }))
        {
            index.generics(lowered, declarations, generics);
            for member in constants {
                index.insert(
                    member.origin.diagnostic_name(member.name_span),
                    declarations.associated_const(member.name_ref),
                    true,
                );
            }
            for member in types {
                index.insert(
                    member.origin.diagnostic_name(member.name_span),
                    declarations.associated_type(member.name_ref),
                    true,
                );
                index.generics(lowered, declarations, &member.generic_params);
            }
        }
        // Canonical targets exclude variant names, alternate-pattern leaves and
        // unavailable bodies; structural traversal does not reclassify any binder.
        for (_, pattern) in module.nodes.patterns() {
            if let PatternKind::Name { local, .. } = pattern.kind {
                index.insert(
                    lowered.module.nodes.local(local).origin.diagnostic_span(),
                    declarations.target(ResolvedName::Local(local)),
                    false,
                );
            }
        }
        for (_, expr) in module.nodes.expressions() {
            if let ExprKind::Closure { params, .. } = &expr.kind {
                for param in params {
                    index.insert(
                        lowered
                            .module
                            .nodes
                            .local(param.local)
                            .origin
                            .diagnostic_span(),
                        declarations.target(ResolvedName::Local(param.local)),
                        false,
                    );
                }
            }
        }
        // Inherited generic handles can visit one site more than once.
        // Keep one entry per canonical identity and coordinate.
        index
            .sites
            .sort_by_key(|site| (site.range.start, site.range.end, site.member));
        index.sites.dedup_by(|a, b| {
            a.range == b.range && a.member == b.member && a.declaration.id == b.declaration.id
        });
        index
    }

    fn insert(&mut self, range: Span, declaration: Option<&'a Declaration>, member: bool) {
        if let Some(declaration) = declaration
            && declaration.source_location().is_some()
            && !declaration.name.is_empty()
            && !declaration.name.starts_with('<')
        {
            self.sites.push(DeclarationSite {
                range,
                declaration,
                member,
            });
        }
    }

    fn generics(
        &mut self,
        lowered: &LoweredModule,
        declarations: &'a Declarations,
        params: &[GenericParamId],
    ) {
        for param in params {
            self.insert(
                lowered
                    .module
                    .generic_parameter(*param)
                    .diagnostic_name_span(),
                declarations.generic_parameter(*param),
                false,
            );
        }
    }
}

/// Cursor selection over declarations and origins belonging to one matching lowering.
#[derive(Debug)]
pub struct DeclarationQueries<'a> {
    index: DeclarationSiteIndex<'a>,
}

impl<'a> DeclarationQueries<'a> {
    /// Builds a query-local site index without checking signatures or bodies.
    pub fn new(lowered: &LoweredModule, declarations: &'a Declarations) -> Self {
        Self {
            index: DeclarationSiteIndex::new(lowered, declarations),
        }
    }

    /// Member names only; an unresolved use cannot select an enclosing declaration.
    pub fn member_at(&self, offset: usize) -> Option<&'a Declaration> {
        self.index
            .sites
            .iter()
            .find(|site| site.member && site.range.start <= offset && offset < site.range.end)
            .map(|site| site.declaration)
    }

    /// Selects in analysis coordinates, not a copied/native physical origin.
    pub fn site_at(&self, offset: usize) -> Option<&'a Declaration> {
        self.index
            .sites
            .iter()
            .filter(|site| site.range.start <= offset && offset < site.range.end)
            .min_by_key(|site| site.range.end - site.range.start)
            .map(|site| site.declaration)
    }
}

/// Editor queries that stop before signature and body checking.
#[derive(Debug, Clone, Copy)]
pub struct DeclarationFileQueries<'a> {
    pub(crate) analysis: &'a FileDeclarations,
}

impl<'a> DeclarationFileQueries<'a> {
    /// Borrows one declaration-stage semantic owner.
    pub fn new(analysis: &'a FileDeclarations) -> Self {
        Self { analysis }
    }

    /// Finds a declared member name without checking signatures or bodies.
    pub fn member_at(&self, offset: usize) -> Option<&'a Declaration> {
        self.analysis.syntax()?;
        DeclarationQueries::new(self.analysis.lowered(), self.analysis.declarations())
            .member_at(offset)
    }
}

/// Documentation queries over retained declaration results, without body checking.
#[derive(Debug, Clone, Copy)]
pub struct DeclarationSnapshotQueries<'a> {
    pub(crate) analysis: &'a DeclarationSnapshot,
}

impl<'a> DeclarationSnapshotQueries<'a> {
    /// Borrows the exact declaration snapshot that owns source identities.
    pub fn new(analysis: &'a DeclarationSnapshot) -> Self {
        Self { analysis }
    }
}

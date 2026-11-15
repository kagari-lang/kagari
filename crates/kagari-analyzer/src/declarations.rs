//! Demand-built declaration-site selection in analysis coordinates.

use kagari_common::span::Span;
use kagari_hir::{
    analysis::declarations::{DeclarationSnapshot, FileDeclarations},
    declarations::{Declaration, Declarations},
    hir::{
        expr::ExprKind,
        item::{Item, behavior::GenericParam},
        pattern::PatternKind,
    },
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
        let module = &lowered.module;
        let map = &lowered.source_map;
        let mut named = |item, name| {
            if let Some(span) = map.item_name_span(item) {
                index.insert(span, declarations.target(name), false);
            }
        };
        for item in &module.functions {
            named(Item::Function(item.id), ResolvedName::Function(item.id));
        }
        for item in &module.consts {
            named(Item::Const(item.id), ResolvedName::Const(item.id));
        }
        for item in &module.statics {
            named(Item::Static(item.id), ResolvedName::Static(item.id));
        }
        for item in &module.modules {
            named(Item::Module(item.id), ResolvedName::Module(item.id));
        }
        for item in &module.type_aliases {
            named(Item::TypeAlias(item.id), ResolvedName::TypeAlias(item.id));
        }
        for item in &module.opaque_types {
            named(Item::OpaqueType(item.id), ResolvedName::OpaqueType(item.id));
        }
        for item in &module.structs {
            named(Item::Struct(item.id), ResolvedName::Struct(item.id));
        }
        for item in &module.enums {
            named(Item::Enum(item.id), ResolvedName::Enum(item.id));
        }
        for item in &module.traits {
            named(Item::Trait(item.id), ResolvedName::Trait(item.id));
        }
        for item in &module.functions {
            index.generics(lowered, declarations, &item.generic_params);
            for param in &item.params {
                index.insert(
                    map.param_span(param.id),
                    declarations.target(ResolvedName::Param(param.id)),
                    false,
                );
            }
        }
        for item in &module.type_aliases {
            index.generics(lowered, declarations, &item.generic_params);
        }
        for item in &module.opaque_types {
            index.generics(lowered, declarations, &item.generic_params);
        }
        for item in &module.structs {
            index.generics(lowered, declarations, &item.generic_params);
            for field in &item.fields {
                index.insert(map.field_span(field.id), declarations.field(field.id), true);
            }
        }
        for item in &module.enums {
            index.generics(lowered, declarations, &item.generic_params);
            for variant in &item.variants {
                index.insert(
                    map.variant_span(variant.id),
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
                    map.type_span(member.name_ref),
                    declarations.associated_const(member.name_ref),
                    true,
                );
            }
            for member in types {
                index.insert(
                    map.type_span(member.name_ref),
                    declarations.associated_type(member.name_ref),
                    true,
                );
                index.generics(lowered, declarations, &member.generic_params);
            }
        }
        // Canonical targets exclude variant names, alternate-pattern leaves and
        // unavailable bodies; structural traversal does not reclassify any binder.
        for (_, pattern) in module.body.patterns() {
            if let PatternKind::Name { local, .. } = pattern.kind {
                index.insert(
                    map.local_span(local),
                    declarations.target(ResolvedName::Local(local)),
                    false,
                );
            }
        }
        for (_, expr) in module.body.expressions() {
            if let ExprKind::Closure { params, .. } = &expr.kind {
                for param in params {
                    index.insert(
                        map.local_span(param.local),
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
        params: &[GenericParam],
    ) {
        for param in params {
            self.insert(
                lowered.source_map.generic_param_span(param.id),
                declarations.generic_parameter(param.id),
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

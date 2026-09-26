//! Declaration and binding identities owned by one semantic analysis.
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicU64, Ordering},
};

use kagari_common::{
    SourceFile, Span,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, FileSpan},
};

use crate::{
    hir::{BodyOwner, FunctionKind},
    lower::LoweredModule,
    resolver::{ResolvedName, ResolvedNames},
};

static NEXT_ANALYSIS: AtomicU64 = AtomicU64::new(1);

/// Distinguishes analyses even when they use the same text revision with different inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnalysisId(u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BindingId {
    pub analysis: AnalysisId,
    pub body: DefinitionId,
    slot: ResolvedName,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeclarationId {
    Definition(DefinitionId),
    GenericParameter {
        owner: DefinitionId,
        position: usize,
    },
    Binding(BindingId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub id: DeclarationId,
    pub name: String,
    pub location: FileSpan,
}

#[derive(Debug, Clone)]
pub struct Declarations {
    pub(crate) imported_types: crate::imports::ImportedTypes,
    pub(crate) names: std::sync::Arc<crate::resolver::NameTable>,
    pub(crate) hosts: std::sync::Arc<crate::host::HostDeclarations>,
    imports: std::sync::Arc<crate::imports::ModuleImports>,
    analysis: AnalysisId,
    targets: HashMap<DeclarationKey, Declaration>,
    identities: HashMap<DeclarationId, DeclarationKey>,
    sites: HashSet<DeclarationKey>,
    impl_identities: HashMap<crate::hir::ImplId, DefinitionId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DeclarationKey {
    Name(ResolvedName),
    Field(crate::hir::FieldId),
    Variant(crate::hir::VariantId),
    GenericParameter(crate::hir::GenericParamId),
}

impl From<ResolvedName> for DeclarationKey {
    fn from(value: ResolvedName) -> Self {
        Self::Name(value)
    }
}

impl Declarations {
    pub(crate) fn standard_trait(
        &self,
        name: &str,
    ) -> Option<crate::builtin::traits::StandardTrait> {
        if let Some(binding) = self.names.lookup(name) {
            return match binding.target()? {
                ResolvedName::StandardTrait(kind) => Some(kind),
                _ => None,
            };
        }
        if let Some((alias, member)) = name.split_once("::")
            && let Some(binding) = self.names.lookup(alias)
        {
            return match binding.target()? {
                ResolvedName::StandardModule(module) => {
                    crate::builtin::traits::in_module(module, member)
                }
                ResolvedName::SourceImport(index) => {
                    match self.imports.resolve_member(index, member, &self.hosts)? {
                        ResolvedName::StandardTrait(kind) => Some(kind),
                        _ => None,
                    }
                }
                _ => None,
            };
        }
        crate::builtin::traits::StandardTrait::from_name(name)
    }

    pub fn impl_identity(&self, id: crate::hir::ImplId) -> Option<&DefinitionId> {
        self.impl_identities.get(&id)
    }

    pub(crate) fn host_type(&self, name: &str) -> Option<crate::host::HostTypeId> {
        let resolved = if let Some(binding) = self.names.lookup(name) {
            binding.target()
        } else if let Some((alias, member)) = name.split_once("::")
            && let Some(binding) = self.names.lookup(alias)
        {
            match binding.target()? {
                ResolvedName::HostModule(module) => self.hosts.resolve_name_in(module, member),
                ResolvedName::SourceImport(index) => {
                    self.imports.resolve_member(index, member, &self.hosts)
                }
                _ => None,
            }
        } else {
            self.hosts.resolve_name(name)
        };
        match resolved? {
            ResolvedName::HostType(id) => Some(id),
            _ => None,
        }
    }
    pub fn variant(&self, id: crate::hir::VariantId) -> Option<&Declaration> {
        self.targets.get(&DeclarationKey::Variant(id))
    }

    /// Member declaration names only, so an unresolved body reference never
    /// accidentally navigates to an enclosing declaration's whole-file span.
    pub fn member_at(&self, offset: usize) -> Option<&Declaration> {
        self.targets
            .iter()
            .filter(|(key, _)| {
                self.sites.contains(key)
                    && matches!(key, DeclarationKey::Field(_) | DeclarationKey::Variant(_))
            })
            .map(|(_, d)| d)
            .find(|d| d.location.range.start <= offset && offset < d.location.range.end)
    }

    /// Declaration-site lookup uses only identifier-sized ranges. Incomplete
    /// names cannot claim surrounding code.
    pub fn site_at(&self, offset: usize) -> Option<&Declaration> {
        self.targets
            .iter()
            .filter(|(key, _)| self.sites.contains(key))
            .map(|(_, declaration)| declaration)
            .filter(|declaration| {
                let range = declaration.location.range;
                range.start <= offset && offset < range.end
            })
            .min_by_key(|declaration| {
                declaration.location.range.end - declaration.location.range.start
            })
    }

    pub fn imported_types(&self) -> &crate::imports::ImportedTypes {
        &self.imported_types
    }
    pub(crate) fn definition(&self, name: ResolvedName) -> Option<&DefinitionId> {
        match &self.target(name)?.id {
            DeclarationId::Definition(id) => Some(id),
            _ => None,
        }
    }
    pub fn definition_target(&self, id: &DefinitionId) -> Option<ResolvedName> {
        match self
            .identities
            .get(&DeclarationId::Definition(id.clone()))?
        {
            DeclarationKey::Name(name) => Some(*name),
            _ => None,
        }
    }
    pub(crate) fn generic_type(
        &self,
        id: crate::hir::GenericParamId,
    ) -> Option<crate::types::GenericParameterType> {
        let declaration = self.generic_parameter(id)?;
        let DeclarationId::GenericParameter { owner, position } = &declaration.id else {
            return None;
        };
        Some(crate::types::GenericParameterType {
            owner: owner.clone(),
            position: *position,
            name: declaration.name.clone(),
        })
    }
    pub fn analysis_id(&self) -> AnalysisId {
        self.analysis
    }

    /// Parameters declared by this owner, in declaration order. Inherited method
    /// binders keep their original owner and are not included here.
    pub fn parameters_of(&self, owner: &DefinitionId) -> Vec<crate::types::GenericParameterType> {
        if let Some(kind) = crate::builtin::traits::StandardTrait::from_id(owner) {
            return kind.contract().generic_params.clone();
        }
        let mut params = self
            .iter()
            .filter_map(|declaration| {
                let DeclarationId::GenericParameter {
                    owner: declared_owner,
                    position,
                } = &declaration.id
                else {
                    return None;
                };
                (declared_owner == owner).then(|| crate::types::GenericParameterType {
                    owner: owner.clone(),
                    position: *position,
                    name: declaration.name.clone(),
                })
            })
            .collect::<Vec<_>>();
        params.sort_by_key(|parameter| parameter.position);
        params
    }

    pub fn target(&self, name: ResolvedName) -> Option<&Declaration> {
        if let ResolvedName::StandardTrait(kind) = name {
            return Some(&kind.contract().declaration);
        }
        self.targets.get(&DeclarationKey::Name(name))
    }

    pub fn field(&self, field: crate::hir::FieldId) -> Option<&Declaration> {
        self.targets.get(&DeclarationKey::Field(field))
    }
    pub fn generic_parameter(&self, id: crate::hir::GenericParamId) -> Option<&Declaration> {
        self.targets.get(&DeclarationKey::GenericParameter(id))
    }

    /// A binding from another analysis is rejected, even if its arena slot coincides.
    pub fn get(&self, id: &DeclarationId) -> Option<&Declaration> {
        self.identities
            .get(id)
            .and_then(|name| self.targets.get(name))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Declaration> {
        self.targets.values()
    }

    pub(crate) fn collect_named(
        source: &SourceFile,
        lowered: &LoweredModule,
        names: &crate::resolver::DeclarationNames,
        cancel: &kagari_common::cancellation::CancellationToken,
    ) -> Self {
        let analysis = AnalysisId(
            NEXT_ANALYSIS
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("analysis identity exhausted"),
        );
        let mut builder = Builder {
            source,
            cancel,
            result: Self {
                imported_types: Default::default(),
                names: names.items.clone(),
                hosts: names.hosts.clone(),
                imports: names.imports.clone(),
                analysis,
                targets: HashMap::new(),
                identities: HashMap::new(),
                sites: HashSet::new(),
                impl_identities: HashMap::new(),
            },
            occurrences: HashMap::new(),
        };
        let module = &lowered.module;
        let map = &lowered.source_map;
        for item in &module.functions {
            if cancel.check().is_err() {
                return builder.result;
            }
            let kind = match item.kind {
                FunctionKind::User => DefinitionKind::Function,
                FunctionKind::TraitMethod | FunctionKind::ImplMethod => continue,
            };
            let owner = builder.definition(
                ResolvedName::Function(item.id),
                &[],
                kind,
                &item.name,
                map.item_declaration_span(crate::hir::Item::Function(item.id)),
                map.item_name_span(crate::hir::Item::Function(item.id))
                    .is_some(),
            );
            builder.generic_params(&owner, &item.generic_params, map);
        }
        for item in &module.consts {
            if item.owner.is_some() {
                continue;
            }
            if cancel.check().is_err() {
                return builder.result;
            }
            builder.definition(
                ResolvedName::Const(item.id),
                &[],
                DefinitionKind::Const,
                &item.name,
                map.item_declaration_span(crate::hir::Item::Const(item.id)),
                map.item_name_span(crate::hir::Item::Const(item.id))
                    .is_some(),
            );
        }
        for item in &module.modules {
            if cancel.check().is_err() {
                return builder.result;
            }
            builder.definition(
                ResolvedName::Module(item.id),
                &[],
                DefinitionKind::Module,
                &item.name,
                map.item_declaration_span(crate::hir::Item::Module(item.id)),
                map.item_name_span(crate::hir::Item::Module(item.id))
                    .is_some(),
            );
        }
        for item in &module.structs {
            if cancel.check().is_err() {
                return builder.result;
            }
            let owner = builder.definition(
                ResolvedName::Struct(item.id),
                &[],
                DefinitionKind::Struct,
                &item.name,
                map.item_declaration_span(crate::hir::Item::Struct(item.id)),
                map.item_name_span(crate::hir::Item::Struct(item.id))
                    .is_some(),
            );
            builder.generic_params(&owner, &item.generic_params, map);
            for field in &item.fields {
                if cancel.check().is_err() {
                    return builder.result;
                }
                let id = builder.identity(&owner.path, DefinitionKind::Field, &field.name);
                builder.insert(
                    DeclarationKey::Field(field.id),
                    DeclarationId::Definition(id),
                    &field.name,
                    map.field_span(field.id),
                    !field.name.is_empty(),
                );
            }
        }
        for item in &module.enums {
            if cancel.check().is_err() {
                return builder.result;
            }
            let owner = builder.definition(
                ResolvedName::Enum(item.id),
                &[],
                DefinitionKind::Enum,
                &item.name,
                map.item_declaration_span(crate::hir::Item::Enum(item.id)),
                map.item_name_span(crate::hir::Item::Enum(item.id))
                    .is_some(),
            );
            builder.generic_params(&owner, &item.generic_params, map);
            for variant in &item.variants {
                if cancel.check().is_err() {
                    return builder.result;
                }
                let id = builder.identity(&owner.path, DefinitionKind::Variant, &variant.name);
                builder.insert(
                    DeclarationKey::Variant(variant.id),
                    DeclarationId::Definition(id),
                    &variant.name,
                    map.variant_span(variant.id),
                    !variant.name.is_empty(),
                );
            }
        }
        for item in &module.traits {
            if cancel.check().is_err() {
                return builder.result;
            }
            let owner = builder.definition(
                ResolvedName::Trait(item.id),
                &[],
                DefinitionKind::Trait,
                &item.name,
                map.item_declaration_span(crate::hir::Item::Trait(item.id)),
                map.item_name_span(crate::hir::Item::Trait(item.id))
                    .is_some(),
            );
            builder.generic_params(&owner, &item.generic_params, map);
            for method in &item.methods {
                if cancel.check().is_err() {
                    return builder.result;
                }
                let method_owner = builder.definition(
                    ResolvedName::Function(method.function),
                    &owner.path,
                    DefinitionKind::Method,
                    &method.name,
                    map.item_declaration_span(crate::hir::Item::Function(method.function)),
                    map.item_name_span(crate::hir::Item::Function(method.function))
                        .is_some(),
                );
                let function = module
                    .functions
                    .iter()
                    .find(|function| function.id == method.function)
                    .expect("trait method function");
                builder.generic_params(&method_owner, &function.generic_params, map);
            }
        }
        for item in &module.impls {
            if cancel.check().is_err() {
                return builder.result;
            }
            let owner = builder.identity(&[], DefinitionKind::Impl, "");
            builder
                .result
                .impl_identities
                .insert(item.id, owner.clone());
            builder.generic_params(&owner, &item.generic_params, map);
            for method in &item.methods {
                if cancel.check().is_err() {
                    return builder.result;
                }
                let method_owner = builder.definition(
                    ResolvedName::Function(method.function),
                    &owner.path,
                    DefinitionKind::Method,
                    &method.name,
                    map.item_declaration_span(crate::hir::Item::Function(method.function)),
                    map.item_name_span(crate::hir::Item::Function(method.function))
                        .is_some(),
                );
                let function = module
                    .functions
                    .iter()
                    .find(|function| function.id == method.function)
                    .expect("impl method function");
                builder.generic_params(&method_owner, &function.generic_params, map);
            }
        }
        for item in &module.consts {
            let owner = match item.owner {
                Some(crate::hir::ConstOwner::Trait(id)) => {
                    builder.result.definition(ResolvedName::Trait(id)).cloned()
                }
                Some(crate::hir::ConstOwner::Impl(id)) => builder.result.impl_identity(id).cloned(),
                None => continue,
            };
            if let Some(owner) = owner {
                builder.definition(
                    ResolvedName::Const(item.id),
                    &owner.path,
                    DefinitionKind::Const,
                    &item.name,
                    map.item_declaration_span(crate::hir::Item::Const(item.id)),
                    !item.name.is_empty(),
                );
            }
        }
        for (members, owner) in module
            .traits
            .iter()
            .filter_map(|item| {
                builder
                    .result
                    .definition(ResolvedName::Trait(item.id))
                    .cloned()
                    .map(|owner| (&item.associated_types, owner))
            })
            .chain(module.impls.iter().filter_map(|item| {
                builder
                    .result
                    .impl_identity(item.id)
                    .cloned()
                    .map(|owner| (&item.associated_types, owner))
            }))
            .collect::<Vec<_>>()
        {
            for member in members {
                builder.generic_params(
                    &crate::types::associated_type_id(&owner, &member.name),
                    &member.generic_params,
                    map,
                );
            }
        }
        builder.result
    }

    pub(crate) fn with_bindings(
        mut self,
        lowered: &LoweredModule,
        names: &ResolvedNames,
        cancel: &kagari_common::cancellation::CancellationToken,
    ) -> Self {
        // Cached named declarations do not extend the lifetime of local handles.
        self.analysis = AnalysisId(
            NEXT_ANALYSIS
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("analysis identity exhausted"),
        );
        let analysis = self.analysis;
        let map = &lowered.source_map;
        let mut builder = Builder {
            source: &lowered.source,
            cancel,
            result: self,
            occurrences: HashMap::new(),
        };
        for scope in names.scopes() {
            if cancel.check().is_err() {
                return builder.result;
            }
            let owner = match scope.owner {
                BodyOwner::Function(id) => ResolvedName::Function(id),
                BodyOwner::Const(id) => ResolvedName::Const(id),
            };
            let DeclarationId::Definition(body) = builder.result.targets[&owner.into()].id.clone()
            else {
                unreachable!("body owner is a definition")
            };
            for binding in &scope.bindings {
                if cancel.check().is_err() {
                    return builder.result;
                }
                let range = match binding.resolved {
                    ResolvedName::Param(id) => map.param_span(id),
                    ResolvedName::Local(id) => map.local_span(id),
                    _ => unreachable!("scope binds parameters and locals"),
                };
                builder.insert(
                    binding.resolved,
                    DeclarationId::Binding(BindingId {
                        analysis,
                        body: body.clone(),
                        slot: binding.resolved,
                    }),
                    &binding.name,
                    range,
                    !binding.name.is_empty() && !binding.name.starts_with('<'),
                );
            }
        }
        builder.result
    }
}

struct Builder<'a> {
    source: &'a SourceFile,
    cancel: &'a kagari_common::cancellation::CancellationToken,
    result: Declarations,
    occurrences: HashMap<(Vec<DefinitionPathSegment>, DefinitionKind, String), u32>,
}

impl Builder<'_> {
    fn generic_params(
        &mut self,
        owner: &DefinitionId,
        params: &[crate::hir::GenericParam],
        map: &crate::source_map::SourceMap,
    ) {
        let mut position = 0;
        for param in params {
            if self.cancel.check().is_err() {
                break;
            }
            let key = DeclarationKey::GenericParameter(param.id);
            // Inherited parameters keep the identity of their trait/impl declaration.
            if self.result.targets.contains_key(&key) {
                continue;
            }
            self.insert(
                key,
                DeclarationId::GenericParameter {
                    owner: owner.clone(),
                    position,
                },
                &param.name,
                map.generic_param_span(param.id),
                !param.name.is_empty(),
            );
            position += 1;
        }
    }
    fn identity(
        &mut self,
        parent: &[DefinitionPathSegment],
        kind: DefinitionKind,
        name: &str,
    ) -> DefinitionId {
        let occurrence = self
            .occurrences
            .entry((parent.to_vec(), kind, name.into()))
            .or_default();
        let mut path = parent.to_vec();
        path.push(DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: *occurrence,
        });
        *occurrence = occurrence
            .checked_add(1)
            .expect("declaration identity exhausted");
        DefinitionId {
            module: self.source.module_identity().clone(),
            path,
        }
    }

    fn definition(
        &mut self,
        key: ResolvedName,
        parent: &[DefinitionPathSegment],
        kind: DefinitionKind,
        name: &str,
        range: Span,
        site: bool,
    ) -> DefinitionId {
        let id = self.identity(parent, kind, name);
        self.insert(
            key,
            DeclarationId::Definition(id.clone()),
            name,
            range,
            site,
        );
        id
    }

    fn insert(
        &mut self,
        key: impl Into<DeclarationKey>,
        id: DeclarationId,
        name: &str,
        range: Span,
        site: bool,
    ) {
        let key = key.into();
        if site {
            self.result.sites.insert(key);
        }
        self.result.identities.insert(id.clone(), key);
        self.result.targets.insert(
            key,
            Declaration {
                id,
                name: name.into(),
                location: FileSpan {
                    file: self.source.origin_id(),
                    revision: self.source.revision(),
                    range,
                },
            },
        );
    }
}

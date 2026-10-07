//! Declaration and binding identities owned by one semantic analysis.
use kagari_types::{collection::CollectionAccess, language::role::LangRole};

use crate::{
    hir::{
        ids::{BodyOwner, FieldId, GenericParamId, ImplId, OpaqueTypeId, TypeRefId, VariantId},
        item::{Item, behavior::GenericParam, function::FunctionKind, storage::ConstOwner},
    },
    host::HostDeclarations,
    imports::{ModuleImportFacts, SourceItem, catalog::NamespaceCatalog, types::ImportedTypes},
    lower::LoweredModule,
    native::NativeTypeKind,
    resolver::{
        resolved::{DeclarationNames, ResolvedName, ResolvedNames},
        table::NameTable,
    },
    source_map::SourceMap,
    types::GenericParameterType,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment,
        map::DefinitionContext,
        reference::DefinitionReference,
        table::{DefinitionId, DefinitionTable},
    },
    span::Span,
};
use kagari_source::{identity::FileSpan, source::SourceFile};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static NEXT_ANALYSIS: AtomicU64 = AtomicU64::new(1);

/// Distinguishes analyses even when they use the same text revision with different inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnalysisId(u64);

/// Analysis-qualified identity of a parameter or local binding for navigation.
///
/// Combines an analysis identity, a scoped owning-body definition and the resolved
/// local slot. Equal source text/slot numbers in another analysis do not identify
/// the same binding. Unlike a portable definition path, this is snapshot-local.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BindingId {
    /// Analysis instance that owns the binding record.
    pub analysis: AnalysisId,
    /// Scoped definition identity of the owning function or constant.
    pub body: DefinitionId,
    /// Parameter/local resolver handle qualified by the enclosing analysis/body.
    slot: ResolvedName,
}

/// Semantic declaration identity, distinguishing nominal definitions, binders and body locals.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeclarationId<I: DefinitionReference = DefinitionPath> {
    /// Portable or scoped identity of a named definition.
    Definition(I),
    /// A binder identified by its declaring owner and ordinal, not spelling.
    GenericParameter {
        /// Definition that declares this generic parameter.
        owner: I,
        /// Zero-based parameter position within that owner.
        position: usize,
    },
    /// A parameter/local binding valid only in its owning analysis.
    Binding(BindingId),
}

/// Semantic identity and authoritative source navigation site for one declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration<I: DefinitionReference = DefinitionPath> {
    /// Named definition, generic binder or analysis-local binding identity.
    pub id: DeclarationId<I>,
    /// Display spelling; identity is carried separately.
    pub name: String,
    /// Physical file/revision and half-open byte range for navigation.
    pub location: FileSpan,
}

/// Declaration identities and source sites collected before and during body analysis.
///
/// ```text
/// resolved local/member handle -> targets -> Declaration { id, name, location }
/// DeclarationId               -> identities -> local/member handle -> targets
/// generic parameter           -> owner definition + ordinal
/// parameter/local binding     -> AnalysisId + scoped body definition + local slot
/// ```
///
/// `collect_named` visits headers/members and builds portable paths with kind/name/
/// occurrence segments. `with_bindings` adds parameters and locals from resolved
/// body scopes. The shared definition context interns paths into a matching table;
/// checked publication maps portable records to its scoped IDs. Internal site ranges
/// use analysis coordinates, which can differ from authoritative copied-source sites.
/// Import lookup tables are shared inputs, not copied into every declaration.
#[derive(Debug, Clone)]
pub struct Declarations<I: DefinitionReference = DefinitionPath> {
    /// Validated language-role identities used by ordinary syntax and protocol checking.
    pub(crate) language_items: BTreeMap<LangRole, I>,
    /// Installed array protocol bridge declarations, when available.
    pub(crate) array_interfaces: BTreeMap<CollectionAccess, I>,
    /// Type/variant projections selected by this module's resolved imports.
    pub(crate) imported_types: ImportedTypes<I>,
    /// Module-level name table used to resolve declaration references.
    pub(crate) names: Arc<NameTable>,
    /// Shared registered host declaration inputs.
    pub(crate) hosts: Arc<HostDeclarations>,
    imports: Arc<ModuleImportFacts>,
    catalog: Arc<NamespaceCatalog>,
    analysis: AnalysisId,
    definitions: DefinitionTable,
    /// Shared definition interning context; publication retains a matching table snapshot.
    context: DefinitionContext,
    /// Local name/member keys to identity and navigation records.
    targets: HashMap<DeclarationKey, Declaration<I>>,
    /// Reverse identity lookup; binding keys include analysis identity.
    identities: HashMap<DeclarationId<I>, DeclarationKey>,
    /// Declaration-site bytes in analysis coordinates, separate from navigation origins.
    site_ranges: HashMap<DeclarationKey, Span>,
    /// Local implementation slots to semantic definition identities.
    impl_identities: HashMap<ImplId, I>,
    native_types: HashMap<OpaqueTypeId, NativeTypeKind<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum DeclarationKey {
    Name(ResolvedName),
    Field(FieldId),
    Variant(VariantId),
    GenericParameter(GenericParamId),
    AssociatedType(TypeRefId),
}

impl From<ResolvedName> for DeclarationKey {
    fn from(value: ResolvedName) -> Self {
        Self::Name(value)
    }
}

impl Declarations {
    /// Collects header/member identities and sites, retaining partial facts if cancellation is observed by the caller.
    pub(crate) fn collect_named(
        source: &SourceFile,
        lowered: &LoweredModule,
        names: &DeclarationNames,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Self {
        let analysis = AnalysisId(
            NEXT_ANALYSIS
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("analysis identity exhausted"),
        );
        let mut builder = Builder {
            source,
            cancel,
            result: Self {
                language_items: BTreeMap::new(),
                array_interfaces: names.imports.array_interfaces.clone(),
                imported_types: Default::default(),
                names: names.items.clone(),
                hosts: names.hosts.clone(),
                imports: names.imports.clone(),
                catalog: names.catalog.clone(),
                analysis,
                definitions: context.snapshot(),
                context: context.clone(),
                targets: HashMap::new(),
                identities: HashMap::new(),
                site_ranges: HashMap::new(),
                impl_identities: HashMap::new(),
                native_types: lowered.native_types.clone(),
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
                map.item_declaration_span(Item::Function(item.id)),
                map.item_name_span(Item::Function(item.id)).is_some(),
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
                map.item_declaration_span(Item::Const(item.id)),
                map.item_name_span(Item::Const(item.id)).is_some(),
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
                map.item_declaration_span(Item::Module(item.id)),
                map.item_name_span(Item::Module(item.id)).is_some(),
            );
        }
        for item in &module.opaque_types {
            if cancel.check().is_err() {
                return builder.result;
            }
            let owner = builder.definition(
                ResolvedName::OpaqueType(item.id),
                &[],
                DefinitionKind::AssociatedType,
                &item.name,
                map.item_declaration_span(Item::OpaqueType(item.id)),
                map.item_name_span(Item::OpaqueType(item.id)).is_some(),
            );
            builder.generic_params(&owner, &item.generic_params, map);
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
                map.item_declaration_span(Item::Struct(item.id)),
                map.item_name_span(Item::Struct(item.id)).is_some(),
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
                map.item_declaration_span(Item::Enum(item.id)),
                map.item_name_span(Item::Enum(item.id)).is_some(),
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
                map.item_declaration_span(Item::Trait(item.id)),
                map.item_name_span(Item::Trait(item.id)).is_some(),
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
                    map.item_declaration_span(Item::Function(method.function)),
                    map.item_name_span(Item::Function(method.function))
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
                    map.item_declaration_span(Item::Function(method.function)),
                    map.item_name_span(Item::Function(method.function))
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
                Some(ConstOwner::Trait(id)) => {
                    builder.result.definition(ResolvedName::Trait(id)).cloned()
                }
                Some(ConstOwner::Impl(id)) => builder.result.impl_identity(id).cloned(),
                None => continue,
            };
            if let Some(owner) = owner {
                builder.definition(
                    ResolvedName::Const(item.id),
                    &owner.path,
                    DefinitionKind::Const,
                    &item.name,
                    map.item_declaration_span(Item::Const(item.id)),
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
                if cancel.check().is_err() {
                    return builder.result;
                }
                let id =
                    builder.identity(&owner.path, DefinitionKind::AssociatedType, &member.name);
                builder.insert(
                    DeclarationKey::AssociatedType(member.name_ref),
                    DeclarationId::Definition(id.clone()),
                    &member.name,
                    map.type_span(member.name_ref),
                    !member.name.is_empty(),
                );
                builder.generic_params(&id, &member.generic_params, map);
            }
        }
        builder.result
    }

    /// Adds resolved parameter/local declarations using this analysis and the owning body's scoped identity.
    pub(crate) fn with_bindings(
        mut self,
        lowered: &LoweredModule,
        names: &ResolvedNames,
        cancel: &CancellationToken,
    ) -> Self {
        // Cached named declarations do not extend the lifetime of local handles.
        // Invalid source identities remain available to recovery queries, but
        // their declaration diagnostic prevents executable adoption.
        if !lowered.source.module_identity().within_path_limit() {
            return self;
        }
        self.analysis = AnalysisId(
            NEXT_ANALYSIS
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("analysis identity exhausted"),
        );
        let analysis = self.analysis;
        let definitions = self.context.clone();
        let map = &lowered.source_map;
        let mut builder = Builder {
            source: &lowered.source,
            cancel,
            result: self,
            occurrences: HashMap::new(),
        };
        for scope in names.scopes() {
            if cancel.check().is_err() {
                break;
            }
            let owner = match scope.owner {
                BodyOwner::Function(id) => ResolvedName::Function(id),
                BodyOwner::Const(id) => ResolvedName::Const(id),
            };
            let DeclarationId::Definition(body) = builder.result.targets[&owner.into()].id.clone()
            else {
                unreachable!("body owner is a definition")
            };
            let body = definitions
                .intern(&body)
                .expect("validated source definition path");
            for binding in &scope.bindings {
                if cancel.check().is_err() {
                    break;
                }
                let range = match binding.resolved.clone() {
                    ResolvedName::Param(id) => map.param_span(id),
                    ResolvedName::Local(id) => map.local_span(id),
                    _ => unreachable!("scope binds parameters and locals"),
                };
                builder.insert(
                    binding.resolved.clone(),
                    DeclarationId::Binding(BindingId {
                        analysis,
                        body,
                        slot: binding.resolved.clone(),
                    }),
                    &binding.name,
                    range,
                    !binding.name.is_empty() && !binding.name.starts_with('<'),
                );
            }
        }
        builder.result.definitions = definitions.snapshot();
        builder.result
    }
}

struct Builder<'a> {
    source: &'a SourceFile,
    cancel: &'a CancellationToken,
    result: Declarations,
    occurrences: HashMap<(Vec<DefinitionPathSegment>, DefinitionKind, String), u32>,
}

impl Builder<'_> {
    fn generic_params(&mut self, owner: &DefinitionPath, params: &[GenericParam], map: &SourceMap) {
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
    ) -> DefinitionPath {
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
        DefinitionPath {
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
    ) -> DefinitionPath {
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
            self.result.site_ranges.insert(key.clone(), range);
        }
        self.result.identities.insert(id.clone(), key.clone());
        self.result.targets.insert(
            key,
            Declaration {
                id,
                name: name.into(),
                location: self.source.span(range).unwrap_or(FileSpan {
                    file: self.source.origin_id(),
                    revision: self.source.revision(),
                    range,
                }),
            },
        );
    }
}

mod mapping;

impl<I: DefinitionReference> Declarations<I> {
    /// Borrows the scoped definition table associated with these records.
    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    /// Clones the installed representation for a local opaque type, if registered.
    pub fn native_type(&self, id: OpaqueTypeId) -> Option<NativeTypeKind<I>> {
        self.native_types.get(&id).cloned()
    }

    /// Borrows the semantic identity assigned to a local implementation block.
    pub fn impl_identity(&self, id: ImplId) -> Option<&I> {
        self.impl_identities.get(&id)
    }

    pub(crate) fn resolve_name(&self, name: &str) -> Option<ResolvedName> {
        self.catalog
            .resolve_name(&self.names, &self.hosts, name, &Default::default())
            .map(|hit| hit.target.resolved(self.names.unit.as_ref()))
    }

    pub(crate) fn resolved_variant(&self, name: ResolvedName) -> Option<&Declaration<I>> {
        if let ResolvedName::Source(source) = &name
            && Some(&source.unit) == self.names.unit.as_ref()
            && let SourceItem::Variant(id) = source.item
        {
            return self.variant(id);
        }
        self.imported_types.variant(name)
    }

    /// Finds a local enum variant declaration by its arena/owner/slot ID.
    pub fn variant(&self, id: VariantId) -> Option<&Declaration<I>> {
        self.targets.get(&DeclarationKey::Variant(id))
    }

    /// Member declaration names only, so an unresolved body reference never
    /// accidentally navigates to an enclosing declaration's whole-file span.
    pub fn member_at(&self, offset: usize) -> Option<&Declaration<I>> {
        self.targets
            .iter()
            .filter(|(key, _)| {
                matches!(
                    key,
                    DeclarationKey::Field(_)
                        | DeclarationKey::Variant(_)
                        | DeclarationKey::AssociatedType(_)
                )
            })
            .find_map(|(key, declaration)| {
                let range = self.site_ranges.get(key)?;
                (range.start <= offset && offset < range.end).then_some(declaration)
            })
    }

    /// Declaration-site lookup uses analysis coordinates, independently of the
    /// authoritative navigation location of a copied source fragment.
    pub fn site_at(&self, offset: usize) -> Option<&Declaration<I>> {
        self.site_ranges
            .iter()
            .filter(|(_, range)| range.start <= offset && offset < range.end)
            .min_by_key(|(_, range)| range.end - range.start)
            .and_then(|(key, _)| self.targets.get(key))
    }

    /// Borrows canonical foreign type/variant declaration projections.
    pub fn imported_types(&self) -> &ImportedTypes<I> {
        &self.imported_types
    }

    pub(crate) fn definition(&self, name: ResolvedName) -> Option<&I> {
        match &self.target(name)?.id {
            DeclarationId::Definition(id) => Some(id),
            _ => None,
        }
    }

    /// Finds a local name target for a definition identity; field/member-only keys return `None`.
    pub fn definition_target(&self, id: &I) -> Option<ResolvedName> {
        match self
            .identities
            .get(&DeclarationId::Definition(id.clone()))?
        {
            DeclarationKey::Name(name) => Some(name.clone()),
            _ => None,
        }
    }

    pub(crate) fn generic_type(&self, id: GenericParamId) -> Option<GenericParameterType<I>> {
        let declaration = self.generic_parameter(id)?;
        let DeclarationId::GenericParameter { owner, position } = &declaration.id else {
            return None;
        };
        Some(GenericParameterType {
            owner: owner.clone(),
            position: *position,
            name: declaration.name.clone(),
        })
    }

    /// Returns the instance identity used to prevent cross-analysis binding lookup.
    pub fn analysis_id(&self) -> AnalysisId {
        self.analysis
    }

    /// Parameters declared by this owner, in declaration order. Inherited method
    /// binders keep their original owner and are not included here.
    pub fn parameters_of(&self, owner: &I) -> Vec<GenericParameterType<I>> {
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
                (declared_owner == owner).then(|| GenericParameterType {
                    owner: owner.clone(),
                    position: *position,
                    name: declaration.name.clone(),
                })
            })
            .collect::<Vec<_>>();
        params.sort_by_key(|parameter| parameter.position);
        params
    }

    /// Finds declaration metadata for a resolved name, including eligible local/imported variants.
    pub fn target(&self, name: ResolvedName) -> Option<&Declaration<I>> {
        self.targets
            .get(&DeclarationKey::Name(name.clone()))
            .or_else(|| self.resolved_variant(name))
    }

    /// Finds a local struct field declaration, or `None` if not recorded.
    pub fn field(&self, field: FieldId) -> Option<&Declaration<I>> {
        self.targets.get(&DeclarationKey::Field(field))
    }

    /// Finds the declaration-site record for a local generic-parameter handle.
    pub fn generic_parameter(&self, id: GenericParamId) -> Option<&Declaration<I>> {
        self.targets.get(&DeclarationKey::GenericParameter(id))
    }

    /// A binding from another analysis is rejected, even if its arena slot coincides.
    pub fn get(&self, id: &DeclarationId<I>) -> Option<&Declaration<I>> {
        self.identities
            .get(id)
            .and_then(|name| self.targets.get(name))
    }

    /// Visits stored declaration records in unspecified hash-map order.
    pub fn iter(&self) -> impl Iterator<Item = &Declaration<I>> {
        self.targets.values()
    }
}

impl<I: DefinitionReference> Declarations<I> {
    /// Shared definition interning context; publication retains a matching table snapshot.
    pub(crate) fn context(&self) -> &DefinitionContext {
        &self.context
    }

    pub(crate) fn publish_definitions(&mut self, definitions: DefinitionTable) {
        self.definitions = definitions;
    }
}

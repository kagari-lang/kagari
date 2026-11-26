//! Declaration and binding identities owned by one semantic analysis.
use kagari_types::{declaration::names::NameNamespace, language::role::LangRole};

use crate::{
    collections::HashMap,
    hir::{
        ids::{
            BodyOwner, FieldId, GenericParamId, HirOwner, ImplId, NativeTypeId, TypeRefId,
            VariantId,
        },
        item::{Module, function::FunctionKind},
        origin::NodeOrigin,
        ty::{DefinitionExposure, TypeKind},
    },
    imports::{
        SourceItem,
        catalog::{
            LookupContext, LookupResult, NamespaceCatalog,
            definitions::{DefinitionLookup, DefinitionSelection},
        },
        types::ImportedTypes,
    },
    language::types::TypeConstructor,
    lower::LoweredModule,
    resolver::{
        resolved::{DeclarationNames, ResolvedName, ResolvedNames},
        table::NameTable,
    },
    types::GenericParameterType,
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{
        DefinitionKind,
        map::DefinitionContext,
        mapping::DefinitionMappingError,
        table::{DefinitionId, DefinitionTable},
    },
    span::Span,
};
use kagari_source::{identity::FileSpan, source::SourceFile};
use std::{
    collections::BTreeMap,
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
pub enum DeclarationId {
    /// Scoped identity of a named definition.
    Definition(DefinitionId),
    /// A binder identified by its declaring owner and ordinal, not spelling.
    GenericParameter {
        /// Definition that declares this generic parameter.
        owner: DefinitionId,
        /// Zero-based parameter position within that owner.
        position: usize,
    },
    /// A parameter/local binding valid only in its owning analysis.
    Binding(BindingId),
}

/// Authoritative attribution of a declaration, not executable installation evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationOrigin {
    /// Exact physical source coordinates, including exported declaration sources.
    Source(FileSpan),
    /// Registered record identity in this declaration's retained definition scope.
    /// No file or authored byte range is available.
    Structured { declaration: DefinitionId },
}

/// Semantic identity and authoritative attribution for one declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    /// Named definition, generic binder or analysis-local binding identity.
    pub id: DeclarationId,
    /// Display spelling; identity is carried separately.
    pub name: String,
    /// Exact source coordinates or structured registration attribution.
    pub origin: DeclarationOrigin,
}

impl Declaration {
    /// Exact authored navigation destination, absent for structured registrations.
    pub fn source_location(&self) -> Option<FileSpan> {
        match self.origin {
            DeclarationOrigin::Source(location) => Some(location),
            DeclarationOrigin::Structured { .. } => None,
        }
    }

    /// Diagnostic anchor only; an empty anchor is not source/navigation evidence.
    pub(crate) fn diagnostic_span(&self) -> Span {
        self.source_location()
            .map(|location| location.range)
            .unwrap_or_default()
    }
}

/// Declaration identities and source sites collected before and during body analysis.
///
/// ```text
/// resolved local/member handle -> targets -> Declaration { id, name, origin }
/// DeclarationId               -> identities -> local/member handle -> targets
/// generic parameter           -> owner definition + ordinal
/// parameter/local binding     -> AnalysisId + scoped body definition + local slot
/// ```
///
/// `collect_named` registers headers/members directly in the shared definition
/// context before signatures are checked. `with_bindings` adds parameters and locals
/// under those existing body identities. Publication retains the matching prefix.
/// Import lookup tables are shared inputs, not copied into every declaration.
#[derive(Debug, Clone)]
pub struct Declarations {
    /// Validated language-role identities used by ordinary syntax and protocol checking.
    pub(crate) language_items: BTreeMap<LangRole, DefinitionId>,
    /// Type/variant projections selected by this module's resolved imports.
    pub(crate) imported_types: ImportedTypes,
    /// Module-level name table used to resolve declaration references.
    pub(crate) names: Arc<NameTable>,
    /// Shared registered host declaration inputs.
    catalog: Arc<NamespaceCatalog>,
    analysis: AnalysisId,
    definitions: DefinitionTable,
    /// Shared definition interning context; publication retains a matching table snapshot.
    context: DefinitionContext,
    /// Local name/member keys to identity and navigation records.
    targets: HashMap<DeclarationKey, Declaration>,
    /// Reverse identity lookup; binding keys include analysis identity.
    identities: HashMap<DeclarationId, DeclarationKey>,
    /// Local implementation slots to semantic definition identities.
    impl_identities: HashMap<ImplId, DefinitionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum DeclarationKey {
    Name(ResolvedName),
    Field(FieldId),
    Variant(VariantId),
    GenericParameter(GenericParamId),
    AssociatedType(TypeRefId),
    AssociatedConst(TypeRefId),
}

impl From<ResolvedName> for DeclarationKey {
    fn from(value: ResolvedName) -> Self {
        Self::Name(value)
    }
}

impl Declarations {
    /// Construct empty recovery facts without assigning an identity to an invalid source module.
    pub(crate) fn empty(names: &DeclarationNames, context: &DefinitionContext) -> Self {
        let analysis = AnalysisId(
            NEXT_ANALYSIS
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("analysis identity exhausted"),
        );
        Self {
            language_items: BTreeMap::new(),
            imported_types: Default::default(),
            names: names.items.clone(),
            catalog: names.catalog.clone(),
            analysis,
            definitions: context.snapshot(),
            context: context.clone(),
            targets: HashMap::default(),
            identities: HashMap::default(),
            impl_identities: HashMap::default(),
        }
    }

    /// Register header/member identities before checking signatures; failed collection publishes no facts.
    pub(crate) fn collect_named(
        source: &SourceFile,
        lowered: &LoweredModule,
        names: &DeclarationNames,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        cancel
            .check()
            .map_err(|_| DefinitionMappingError::Cancelled)?;
        let root = context.intern_root(source.module_identity())?;
        let mut builder = Builder {
            source: lowered.source_surface().map(|_| source),
            module: &lowered.module,
            cancel,
            result: Self::empty(names, context),
            occurrences: HashMap::default(),
        };
        let module = &lowered.module;
        for item in &module.functions {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            let kind = match item.kind {
                FunctionKind::User => DefinitionKind::Function,
                FunctionKind::Local { .. }
                | FunctionKind::TraitMethod
                | FunctionKind::ImplMethod => continue,
            };
            let owner = builder.definition(
                ResolvedName::Function(item.id),
                root,
                kind,
                &item.name,
                item.origin,
                item.origin.diagnostic_name(item.name_span),
            )?;
            builder.generic_params(&owner, &item.generic_params);
        }
        for item in &module.consts {
            if item.owner.is_some() {
                continue;
            }
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            builder.definition(
                ResolvedName::Const(item.id),
                root,
                DefinitionKind::Const,
                &item.name,
                item.origin,
                item.origin.diagnostic_name(item.name_span),
            )?;
        }
        for item in &module.statics {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            builder.definition(
                ResolvedName::Static(item.id),
                root,
                DefinitionKind::Static,
                &item.name,
                item.origin,
                item.origin.diagnostic_name(item.name_span),
            )?;
        }
        for item in &module.modules {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            builder.definition(
                ResolvedName::Module(item.id),
                root,
                DefinitionKind::Module,
                &item.name,
                item.origin,
                item.origin.diagnostic_name(item.name_span),
            )?;
        }
        for item in &module.type_aliases {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            let owner = builder.definition(
                ResolvedName::TypeAlias(item.id),
                root,
                DefinitionKind::TypeAlias,
                &item.name,
                item.origin,
                item.origin.diagnostic_name(item.name_span),
            )?;
            builder.generic_params(&owner, &item.generic_params);
        }
        for item in &module.native_types {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            let owner = builder.definition(
                ResolvedName::NativeType(item.id),
                root,
                if item.is_host {
                    DefinitionKind::Struct
                } else {
                    DefinitionKind::AssociatedType
                },
                &item.name,
                item.origin,
                item.origin.diagnostic_name(item.name_span),
            )?;
            builder.generic_params(&owner, &item.generic_params);
            for field in &item.fields {
                cancel
                    .check()
                    .map_err(|_| DefinitionMappingError::Cancelled)?;
                let id = builder.identity(owner, DefinitionKind::Field, &field.name)?;
                builder.insert(
                    DeclarationKey::Field(field.id),
                    DeclarationId::Definition(id),
                    &field.name,
                    field.origin,
                    field.diagnostic_name_span(),
                );
            }
        }
        for item in &module.structs {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            let owner = builder.definition(
                ResolvedName::Struct(item.id),
                root,
                DefinitionKind::Struct,
                &item.name,
                item.origin,
                item.origin.diagnostic_name(item.name_span),
            )?;
            builder.generic_params(&owner, &item.generic_params);
            for field in &item.fields {
                cancel
                    .check()
                    .map_err(|_| DefinitionMappingError::Cancelled)?;
                let id = builder.identity(owner, DefinitionKind::Field, &field.name)?;
                builder.insert(
                    DeclarationKey::Field(field.id),
                    DeclarationId::Definition(id),
                    &field.name,
                    field.origin,
                    field.diagnostic_name_span(),
                );
            }
        }
        for item in &module.enums {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            let owner = builder.definition(
                ResolvedName::Enum(item.id),
                root,
                DefinitionKind::Enum,
                &item.name,
                item.origin,
                item.origin.diagnostic_name(item.name_span),
            )?;
            builder.generic_params(&owner, &item.generic_params);
            for variant in &item.variants {
                cancel
                    .check()
                    .map_err(|_| DefinitionMappingError::Cancelled)?;
                let id = builder.identity(owner, DefinitionKind::Variant, &variant.name)?;
                builder.insert(
                    DeclarationKey::Variant(variant.id),
                    DeclarationId::Definition(id),
                    &variant.name,
                    variant.origin,
                    variant.diagnostic_name_span(),
                );
            }
        }
        for item in &module.traits {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            let owner = builder.definition(
                ResolvedName::Trait(item.id),
                root,
                DefinitionKind::Trait,
                &item.name,
                item.origin,
                item.origin.diagnostic_name(item.name_span),
            )?;
            builder.generic_params(&owner, &item.generic_params);
            for method in &item.methods {
                cancel
                    .check()
                    .map_err(|_| DefinitionMappingError::Cancelled)?;
                let function = &module.functions[method.function.index()];
                let method_owner = builder.definition(
                    ResolvedName::Function(method.function),
                    owner,
                    DefinitionKind::Method,
                    &method.name,
                    function.origin,
                    function.origin.diagnostic_name(function.name_span),
                )?;
                builder.generic_params(&method_owner, &function.generic_params);
            }
        }
        for item in &module.impls {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            let owner = builder.identity(root, DefinitionKind::Impl, "")?;
            builder.result.impl_identities.insert(item.id, owner);
            builder.generic_params(&owner, &item.generic_params);
            for method in &item.methods {
                cancel
                    .check()
                    .map_err(|_| DefinitionMappingError::Cancelled)?;
                let function = &module.functions[method.function.index()];
                let method_owner = builder.definition(
                    ResolvedName::Function(method.function),
                    owner,
                    DefinitionKind::Method,
                    &method.name,
                    function.origin,
                    function.origin.diagnostic_name(function.name_span),
                )?;
                builder.generic_params(&method_owner, &function.generic_params);
            }
        }
        for (members, owner) in module
            .traits
            .iter()
            .filter_map(|item| {
                builder
                    .result
                    .definition(ResolvedName::Trait(item.id))
                    .copied()
                    .map(|owner| (&item.associated_consts, owner))
            })
            .chain(module.impls.iter().filter_map(|item| {
                builder
                    .result
                    .impl_identity(item.id)
                    .copied()
                    .map(|owner| (&item.associated_consts, owner))
            }))
            .collect::<Vec<_>>()
        {
            for member in members {
                let id = builder.identity(owner, DefinitionKind::Const, &member.name)?;
                builder.insert(
                    DeclarationKey::AssociatedConst(member.name_ref),
                    DeclarationId::Definition(id),
                    &member.name,
                    member.origin,
                    member.origin.diagnostic_name(member.name_span),
                );
                if let Some(initializer) = member.initializer {
                    let item = module.constant(initializer);
                    // Header and initializer share identity; reverse lookup retains the
                    // executable initializer's local handle rather than its header alias.
                    builder.insert(
                        ResolvedName::Const(initializer),
                        DeclarationId::Definition(id),
                        &member.name,
                        item.origin,
                        item.origin.diagnostic_name(item.name_span),
                    );
                }
            }
        }
        for function in &module.functions {
            let FunctionKind::Local { block } = function.kind else {
                continue;
            };
            let parent = match block.owner() {
                HirOwner::Body(BodyOwner::Function(id)) => builder
                    .result
                    .definition(ResolvedName::Function(id))
                    .copied(),
                HirOwner::Body(BodyOwner::Const(id)) => {
                    builder.result.definition(ResolvedName::Const(id)).copied()
                }
                HirOwner::Body(BodyOwner::Static(id)) => {
                    builder.result.definition(ResolvedName::Static(id)).copied()
                }
                _ => None,
            }
            .ok_or(DefinitionMappingError::InvalidContract)?;
            let owner = builder.definition(
                ResolvedName::Function(function.id),
                parent,
                DefinitionKind::Function,
                &function.name,
                function.origin,
                function.origin.diagnostic_name(function.name_span),
            )?;
            builder.generic_params(&owner, &function.generic_params);
        }
        for (members, owner) in module
            .traits
            .iter()
            .filter_map(|item| {
                builder
                    .result
                    .definition(ResolvedName::Trait(item.id))
                    .copied()
                    .map(|owner| (&item.associated_types, owner))
            })
            .chain(module.impls.iter().filter_map(|item| {
                builder
                    .result
                    .impl_identity(item.id)
                    .copied()
                    .map(|owner| (&item.associated_types, owner))
            }))
            .collect::<Vec<_>>()
        {
            for member in members {
                cancel
                    .check()
                    .map_err(|_| DefinitionMappingError::Cancelled)?;
                let id = builder.identity(owner, DefinitionKind::AssociatedType, &member.name)?;
                builder.insert(
                    DeclarationKey::AssociatedType(member.name_ref),
                    DeclarationId::Definition(id),
                    &member.name,
                    member.origin,
                    member.origin.diagnostic_name(member.name_span),
                );
                builder.generic_params(&id, &member.generic_params);
            }
        }
        cancel
            .check()
            .map_err(|_| DefinitionMappingError::Cancelled)?;
        builder.result.definitions = context.snapshot();
        Ok(builder.result)
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
        let mut builder = Builder {
            source: lowered.source_surface().map(AsRef::as_ref),
            module: &lowered.module,
            cancel,
            result: self,
            occurrences: HashMap::default(),
        };
        for binding in names.bindings() {
            if cancel.check().is_err() {
                break;
            }
            let owner = match binding.owner {
                BodyOwner::Function(id) => ResolvedName::Function(id),
                BodyOwner::Const(id) => ResolvedName::Const(id),
                BodyOwner::Static(id) => ResolvedName::Static(id),
            };
            let DeclarationId::Definition(body) = builder.result.targets[&owner.into()].id.clone()
            else {
                unreachable!("body owner is a definition")
            };
            let (origin, range) = match binding.resolved {
                ResolvedName::Param(id) => {
                    let param = lowered.module.parameter(id);
                    (param.origin, param.binding_span())
                }
                ResolvedName::Local(id) => {
                    let origin = lowered.module.nodes.local(id).origin;
                    (origin, origin.diagnostic_span())
                }
                ResolvedName::Function(_) => continue,
                _ => unreachable!("directory binds parameters, locals and local functions"),
            };
            builder.insert(
                binding.resolved,
                DeclarationId::Binding(BindingId {
                    analysis,
                    body,
                    slot: binding.resolved,
                }),
                &binding.name,
                origin,
                range,
            );
        }
        builder.result
    }
}

struct Builder<'a> {
    source: Option<&'a SourceFile>,
    module: &'a Module,
    cancel: &'a CancellationToken,
    result: Declarations,
    occurrences: HashMap<(DefinitionId, DefinitionKind, String), u32>,
}

impl Builder<'_> {
    fn generic_params(&mut self, owner: &DefinitionId, params: &[GenericParamId]) {
        let mut position = 0;
        for param in params {
            if self.cancel.check().is_err() {
                break;
            }
            let key = DeclarationKey::GenericParameter(*param);
            // Inherited parameters keep the identity of their trait/impl declaration.
            if self.result.targets.contains_key(&key) {
                continue;
            }
            let param = self.module.generic_parameter(*param);
            self.insert(
                key,
                DeclarationId::GenericParameter {
                    owner: *owner,
                    position,
                },
                &param.name,
                param.origin,
                param.diagnostic_name_span(),
            );
            position += 1;
        }
    }

    fn identity(
        &mut self,
        parent: DefinitionId,
        kind: DefinitionKind,
        name: &str,
    ) -> Result<DefinitionId, DefinitionMappingError> {
        self.cancel
            .check()
            .map_err(|_| DefinitionMappingError::Cancelled)?;
        let occurrence = self
            .occurrences
            .entry((parent, kind, name.into()))
            .or_default();
        let id = self
            .result
            .context
            .intern_child(parent, kind, name, *occurrence)?;
        *occurrence = occurrence
            .checked_add(1)
            .ok_or(DefinitionMappingError::LimitExceeded)?;
        Ok(id)
    }

    fn definition(
        &mut self,
        key: ResolvedName,
        parent: DefinitionId,
        kind: DefinitionKind,
        name: &str,
        origin: NodeOrigin,
        range: Span,
    ) -> Result<DefinitionId, DefinitionMappingError> {
        let id = self.identity(parent, kind, name)?;
        self.insert(key, DeclarationId::Definition(id), name, origin, range);
        Ok(id)
    }

    fn insert(
        &mut self,
        key: impl Into<DeclarationKey>,
        id: DeclarationId,
        name: &str,
        origin: NodeOrigin,
        range: Span,
    ) {
        let key = key.into();

        let origin = match origin {
            NodeOrigin::Source(_) => DeclarationOrigin::Source(
                self.source
                    .expect("structured declaration must retain registration attribution")
                    .span(range)
                    .expect("declaration range must belong to its retained source"),
            ),
            NodeOrigin::Structured { declaration } => DeclarationOrigin::Structured { declaration },
        };
        self.result.identities.insert(id.clone(), key.clone());
        self.result.targets.insert(
            key,
            Declaration {
                id,
                name: name.into(),
                origin,
            },
        );
    }
}

mod mapping;

impl Declarations {
    /// Borrows the scoped definition table associated with these records.
    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    pub(crate) fn native_type(&self, id: NativeTypeId) -> Option<TypeConstructor> {
        let declaration = *self.definition(ResolvedName::NativeType(id))?;
        Some(TypeConstructor::declaration(
            declaration,
            self.parameters_of(&declaration).len(),
            self.definitions.resolve(declaration).ok()?.last()?.kind == DefinitionKind::Struct,
            &self.definitions,
        ))
    }

    /// Borrows the semantic identity assigned to a local implementation block.
    pub fn impl_identity(&self, id: ImplId) -> Option<&DefinitionId> {
        self.impl_identities.get(&id)
    }

    pub(crate) fn resolve_name(
        &self,
        name: &str,
        namespace: NameNamespace,
    ) -> Option<ResolvedName> {
        self.catalog
            .resolve_name(&self.names, name, namespace, &Default::default())
            .map(|hit| hit.target.resolved(self.names.unit.as_ref()))
    }

    /// Distinguish denied access from a missing or malformed trait after normal
    /// resolution (including public re-exports for an exact identity) failed.
    pub(crate) fn trait_path_inaccessible(
        &self,
        kind: &TypeKind,
        cancel: &CancellationToken,
    ) -> bool {
        let category = NameNamespace::Type;
        let lookup = match kind {
            TypeKind::Named(name) | TypeKind::Generic { name, .. } => {
                self.catalog
                    .lookup_name(&self.names, name, category, cancel)
            }
            TypeKind::Definition {
                declaration,
                exposure,
                ..
            } => {
                let (Some(unit), Ok(view)) =
                    (&self.names.unit, self.definitions.resolve(*declaration))
                else {
                    return false;
                };
                let route = exposure
                    .clone()
                    .unwrap_or_else(|| DefinitionExposure::defining(view));
                self.catalog.lookup_exposure(
                    &LookupContext::new(&unit.module),
                    &route,
                    category,
                    cancel,
                )
            }
            _ => return false,
        };
        matches!(lookup, Ok(LookupResult::Inaccessible(_)))
    }

    /// Resolve an exact structured reference through the same access-checked catalog.
    /// The caller must still verify the selected definition and its semantic eligibility.
    pub(crate) fn resolve_definition(
        &self,
        declaration: DefinitionId,
        namespace: NameNamespace,
        cancel: &CancellationToken,
    ) -> Result<Option<ResolvedName>, Cancelled> {
        Ok(self
            .resolve_definition_exposure(declaration, namespace, None, cancel)?
            .map(|selected| selected.hit.target.resolved(self.names.unit.as_ref())))
    }

    /// Select or validate the exact reference's exposure in this catalog generation.
    /// The returned route is resolution evidence, never a substitute for type checking.
    pub(crate) fn resolve_definition_exposure(
        &self,
        declaration: DefinitionId,
        namespace: NameNamespace,
        exposure: Option<&DefinitionExposure>,
        cancel: &CancellationToken,
    ) -> Result<Option<DefinitionSelection>, Cancelled> {
        let Some(unit) = self.names.unit.as_ref() else {
            return Ok(None);
        };
        self.catalog.resolve_definition(
            &LookupContext::new(&unit.module),
            DefinitionLookup {
                declaration,
                definitions: &self.definitions,
                namespace,
                exposure,
            },
            |target| {
                let resolved = target.resolved(self.names.unit.as_ref());
                let id = self.definition(resolved).copied().or_else(|| {
                    let imported = self.imported_types.resolved(resolved)?;
                    match imported.declaration.id {
                        DeclarationId::Definition(id) => Some(id),
                        _ => None,
                    }
                });
                id == Some(declaration)
            },
            cancel,
        )
    }

    pub(crate) fn resolved_variant(&self, name: ResolvedName) -> Option<&Declaration> {
        if let ResolvedName::Source(source) = &name
            && Some(&source.unit) == self.names.unit.as_ref()
            && let SourceItem::Variant(id) = source.item
        {
            return self.variant(id);
        }
        self.imported_types.variant(name)
    }

    /// Borrow an associated type header by its retained source type-name handle.
    pub fn associated_type(&self, name: TypeRefId) -> Option<&Declaration> {
        self.targets.get(&DeclarationKey::AssociatedType(name))
    }

    /// Borrow an associated constant header, including a required member without an initializer.
    pub fn associated_const(&self, name: TypeRefId) -> Option<&Declaration> {
        self.targets.get(&DeclarationKey::AssociatedConst(name))
    }

    /// Finds a local enum variant declaration by its arena/owner/slot ID.
    pub fn variant(&self, id: VariantId) -> Option<&Declaration> {
        self.targets.get(&DeclarationKey::Variant(id))
    }

    /// Borrows canonical foreign type/variant declaration projections.
    pub fn imported_types(&self) -> &ImportedTypes {
        &self.imported_types
    }

    /// Return the authored enclosing owner when a named local has an ambiguous
    /// same-name sibling. Only declarations in this analysis participate; stale
    /// entries in the append-only definition context do not create ambiguity.
    pub fn ambiguous_local_owner(&self, name: ResolvedName) -> Option<DefinitionId> {
        let id = *self.definition(name)?;
        let view = self.definitions.resolve(id).ok()?;
        let member = view.last()?;
        if member.kind != DefinitionKind::Function {
            return None;
        }
        let owner = self.definitions.parent(id).ok()??;
        let parent = self.definitions.resolve(owner).ok()?.last()?;
        if !matches!(
            parent.kind,
            DefinitionKind::Function
                | DefinitionKind::Method
                | DefinitionKind::Const
                | DefinitionKind::Static
        ) {
            return None;
        }
        let duplicate =
            self.definitions
                .lookup_child(owner, DefinitionKind::Function, member.name, 1)?;
        self.definition_target(&duplicate).map(|_| owner)
    }

    /// Resolves a selected name to its canonical definition ID in this table.
    pub fn definition(&self, name: ResolvedName) -> Option<&DefinitionId> {
        match &self.target(name)?.id {
            DeclarationId::Definition(id) => Some(id),
            _ => None,
        }
    }

    /// Finds a local name target for a definition identity; field/member-only keys return `None`.
    pub fn definition_target(&self, id: &DefinitionId) -> Option<ResolvedName> {
        match self.identities.get(&DeclarationId::Definition(*id))? {
            DeclarationKey::Name(name) => Some(*name),
            _ => None,
        }
    }

    pub(crate) fn generic_type(&self, id: GenericParamId) -> Option<GenericParameterType> {
        let declaration = self.generic_parameter(id)?;
        let DeclarationId::GenericParameter { owner, position } = &declaration.id else {
            return None;
        };
        Some(GenericParameterType {
            owner: *owner,
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
    pub fn parameters_of(&self, owner: &DefinitionId) -> Vec<GenericParameterType> {
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
                    owner: *owner,
                    position: *position,
                    name: declaration.name.clone(),
                })
            })
            .collect::<Vec<_>>();
        params.sort_by_key(|parameter| parameter.position);
        params
    }

    /// Finds declaration metadata for a resolved name, including eligible local/imported variants.
    pub fn target(&self, name: ResolvedName) -> Option<&Declaration> {
        self.targets
            .get(&DeclarationKey::Name(name))
            .or_else(|| self.resolved_variant(name))
    }

    /// Finds a local struct field declaration, or `None` if not recorded.
    pub fn field(&self, field: FieldId) -> Option<&Declaration> {
        self.targets.get(&DeclarationKey::Field(field))
    }

    /// Finds the declaration-site record for a local generic-parameter handle.
    pub fn generic_parameter(&self, id: GenericParamId) -> Option<&Declaration> {
        self.targets.get(&DeclarationKey::GenericParameter(id))
    }

    /// A binding from another analysis is rejected, even if its arena slot coincides.
    pub fn get(&self, id: &DeclarationId) -> Option<&Declaration> {
        self.identities
            .get(id)
            .and_then(|name| self.targets.get(name))
    }

    /// Visits stored declaration records in unspecified hash-map order.
    pub fn iter(&self) -> impl Iterator<Item = &Declaration> {
        self.targets.values()
    }
}

impl Declarations {
    /// Shared definition interning context; publication retains a matching table snapshot.
    pub(crate) fn context(&self) -> &DefinitionContext {
        &self.context
    }

    pub(crate) fn publish_definitions(&mut self, definitions: DefinitionTable) {
        self.definitions = definitions;
    }
}

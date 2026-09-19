//! Declaration and binding identities owned by one semantic analysis.
use std::{
    collections::HashMap,
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
    analysis: AnalysisId,
    targets: HashMap<DeclarationKey, Declaration>,
    identities: HashMap<DeclarationId, DeclarationKey>,
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
    pub fn variant(&self, id: crate::hir::VariantId) -> Option<&Declaration> {
        self.targets.get(&DeclarationKey::Variant(id))
    }

    /// Member declaration names only, so an unresolved body reference never
    /// accidentally navigates to an enclosing declaration's whole-file span.
    pub fn member_at(&self, offset: usize) -> Option<&Declaration> {
        self.targets
            .iter()
            .filter(|(key, _)| matches!(key, DeclarationKey::Field(_) | DeclarationKey::Variant(_)))
            .map(|(_, d)| d)
            .find(|d| d.location.range.start <= offset && offset < d.location.range.end)
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
    pub(crate) fn definition_target(&self, id: &DefinitionId) -> Option<ResolvedName> {
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
        names: std::sync::Arc<crate::resolver::NameTable>,
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
                names,
                analysis,
                targets: HashMap::new(),
                identities: HashMap::new(),
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
                FunctionKind::ModuleInit => DefinitionKind::ModuleInit,
                FunctionKind::TraitMethod | FunctionKind::ImplMethod => continue,
            };
            let owner = builder.definition(
                ResolvedName::Function(item.id),
                &[],
                kind,
                &item.name,
                map.function_span(item.id),
            );
            builder.generic_params(&owner, &item.generic_params, map);
        }
        for item in &module.consts {
            if cancel.check().is_err() {
                return builder.result;
            }
            builder.definition(
                ResolvedName::Const(item.id),
                &[],
                DefinitionKind::Const,
                &item.name,
                map.const_span(item.id),
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
                map.module_span(item.id),
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
                map.struct_span(item.id),
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
                map.enum_span(item.id),
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
                map.trait_span(item.id),
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
                    map.function_span(method.function),
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
                    map.function_span(method.function),
                );
                let function = module
                    .functions
                    .iter()
                    .find(|function| function.id == method.function)
                    .expect("impl method function");
                builder.generic_params(&method_owner, &function.generic_params, map);
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
    ) -> DefinitionId {
        let id = self.identity(parent, kind, name);
        self.insert(key, DeclarationId::Definition(id.clone()), name, range);
        id
    }

    fn insert(
        &mut self,
        key: impl Into<DeclarationKey>,
        id: DeclarationId,
        name: &str,
        range: Span,
    ) {
        let key = key.into();
        self.result.identities.insert(id.clone(), key);
        self.result.targets.insert(
            key,
            Declaration {
                id,
                name: name.into(),
                location: FileSpan {
                    file: self.source.id(),
                    revision: self.source.revision(),
                    range,
                },
            },
        );
    }
}

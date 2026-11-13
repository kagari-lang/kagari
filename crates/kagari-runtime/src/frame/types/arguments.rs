//! Closed type arguments retain the lexical scope that supplied their layouts.
use kagari_common::identity::{
    mapping::DefinitionMappingError, scoped::DefinitionReferences, table::DefinitionTable,
};
#[cfg(test)]
mod identity_tests;
mod nominal;
pub(crate) mod variants;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{
        arguments::variants::PreparedEnumMember,
        bindings::TypeBindings,
        compatibility::{TypeIdentity, TypeView},
    },
    host::HostRegistryId,
    module::{EnumVariantRef, ExecutableModule, layout_admission::NominalAdmission},
};
use kagari_types::{declaration::verify::types_in_scope, ty::Ty};
use std::{
    slice,
    sync::{Arc, OnceLock},
};

#[derive(Debug, Clone)]
pub struct TypeArgument {
    data: Arc<TypeArgumentData>,
}

#[derive(Debug)]
struct TypeArgumentData {
    registry: Option<HostRegistryId>,
    ty: Ty,
    definitions: DefinitionTable,
    origin: Option<Arc<TypeOrigin>>,
    parameters: OnceLock<Result<Vec<TypeArgument>, RuntimeError>>,
    variants: OnceLock<Result<Vec<PreparedEnumMember>, RuntimeError>>,
    identity: OnceLock<Option<Arc<TypeIdentity>>>,
    admission: OnceLock<Box<NominalAdmission>>,
}

#[derive(Debug)]
struct TypeOrigin {
    expression: Ty,
    scope: Arc<TypeScope>,
}

#[derive(Debug)]
struct TypeScope {
    owner: ExecutableModule,
    environment: Option<Arc<TypeBindings>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ScopedSignature {
    pub(crate) params: Vec<TypeArgument>,
    pub(crate) result: TypeArgument,
}

impl TypeArgument {
    pub(crate) fn view<'a>(&'a self, owner: &'a ExecutableModule) -> TypeView<'a> {
        match &self.data.origin {
            Some(origin) => TypeView::prepared(
                &origin.expression,
                &self.data.ty,
                &origin.scope.owner,
                origin.scope.environment.as_deref(),
            ),
            None => TypeView::prepared(&self.data.ty, &self.data.ty, owner, None),
        }
        .with_closed_scope(&self.data.definitions)
    }

    /// Exact immutable type/provenance identity, prepared once per supplied argument.
    pub(crate) fn identity(&self, fallback: &ExecutableModule) -> Option<Arc<TypeIdentity>> {
        // Arguments without origins contain no nominal layouts, so their identity
        // is independent of the fallback. Nominal arguments retain their own scope.
        self.data
            .identity
            .get_or_init(|| self.view(fallback).identity().map(Arc::new))
            .clone()
    }

    pub(crate) fn derive(
        &self,
        runtime: &Runtime,
        fallback: &ExecutableModule,
        derive: impl FnOnce(&Ty) -> Option<Ty>,
    ) -> Result<Self, RuntimeError> {
        let (expression, owner, environment) = match &self.data.origin {
            Some(origin) => (
                &origin.expression,
                &origin.scope.owner,
                origin.scope.environment.clone(),
            ),
            None => (&self.data.ty, fallback, None),
        };
        let view = TypeView::new(expression, owner, environment.as_deref())
            .normalized()
            .ok_or_else(|| RuntimeError::module_validation("derived type scope"))?;
        let ty = derive(view.ty)
            .ok_or_else(|| RuntimeError::module_validation("derived type expression"))?;
        runtime
            .type_arguments(
                view.owner,
                view.environment.cloned().map(Arc::new),
                slice::from_ref(&ty),
            )?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("derived type scope"))
    }

    pub(crate) fn parameter(
        &self,
        runtime: &Runtime,
        fallback: &ExecutableModule,
        index: usize,
    ) -> Result<Self, RuntimeError> {
        self.validate(runtime)?;
        self.data
            .parameters
            .get_or_init(|| {
                (0..)
                    .map_while(|position| type_parameter(self.ty(), position).map(|_| position))
                    .map(|position| {
                        self.derive(runtime, fallback, |ty| {
                            type_parameter(ty, position).cloned()
                        })
                    })
                    .collect()
            })
            .as_ref()
            .map_err(Clone::clone)?
            .get(index)
            .cloned()
            .ok_or_else(|| RuntimeError::module_validation("derived type parameter"))
    }

    pub(crate) fn prepared_variants(
        &self,
        prepare: impl FnOnce() -> Result<Vec<EnumVariantRef>, RuntimeError>,
    ) -> Result<&[PreparedEnumMember], RuntimeError> {
        let variants = self
            .data
            .variants
            .get_or_init(|| {
                prepare()?
                    .into_iter()
                    .map(PreparedEnumMember::new)
                    .collect()
            })
            .as_deref()
            .map_err(Clone::clone)?;
        if self.data.admission.get().is_none()
            && let Some(admission) = variants
                .first()
                .and_then(|member| NominalAdmission::enumeration(member.layout()))
        {
            let _ = self.data.admission.set(Box::new(admission));
        }
        Ok(variants)
    }

    pub fn ty(&self) -> &Ty {
        &self.data.ty
    }

    pub fn definitions(&self) -> &DefinitionTable {
        &self.data.definitions
    }

    pub(crate) fn has_origin(&self) -> bool {
        self.data.origin.is_some()
    }

    pub(crate) fn validate(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        if self
            .data
            .registry
            .is_some_and(|registry| registry != runtime.host.owner())
        {
            return Err(RuntimeError::module_validation(
                "foreign type argument registry",
            ));
        }
        if let Some(origin) = &self.data.origin
            && !origin.scope.owner.belongs_to(runtime.host.owner())
        {
            return Err(RuntimeError::module_validation(
                "foreign type argument scope",
            ));
        }
        // Construction checks the immutable accepted closed prefix. Its local
        // registry provenance is sufficient; later writer growth grants nothing.
        Ok(())
    }

    pub(crate) fn validate_for(
        &self,
        runtime: &Runtime,
        owner: &ExecutableModule,
    ) -> Result<(), RuntimeError> {
        self.validate(runtime)?;
        if !self.view(owner).belongs_to_installation(owner) {
            return Err(RuntimeError::module_validation(
                "type argument belongs to another installation",
            ));
        }
        Ok(())
    }
}

pub(crate) fn type_parameter(ty: &Ty, index: usize) -> Option<&Ty> {
    match ty {
        Ty::Struct(nominal)
        | Ty::Enum(nominal)
        | Ty::NativeObject(nominal)
        | Ty::Trait(nominal) => nominal.arguments.get(index),
        Ty::Tuple(items) => items.get(index),
        Ty::Array(item) | Ty::Range(item, _) | Ty::Reference { target: item, .. } if index == 0 => {
            Some(item)
        }

        _ => None,
    }
}

fn contains_scoped_nominal(ty: &Ty) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            // Even shared native traits retain the publication that proved their
            // origin. Their identity is shared, but later registration cannot
            // retroactively reclassify an older script-trait argument.
            Ty::Struct(_) | Ty::Enum(_) | Ty::Trait(_) => return true,
            Ty::Tuple(types) => pending.extend(types),
            Ty::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            Ty::NativeObject(ty) => {
                pending.extend(&ty.arguments);
                pending.extend(ty.associated_types.values());
            }
            Ty::Array(ty) | Ty::Range(ty, _) => pending.push(ty),
            Ty::Reference { target, .. } => pending.push(target),

            _ => {}
        }
    }
    false
}

impl Runtime {
    /// Prepare concrete scoped arguments against a specific loaded generation.
    /// Every identity must belong to the loaded module's retained definition prefix;
    /// this does not import foreign metadata or admit later definitions retroactively.
    pub fn resolve_type_arguments(
        &self,
        owner: &ExecutableModule,
        types: &[Ty],
    ) -> Result<Vec<TypeArgument>, RuntimeError> {
        let cancel = Default::default();
        for ty in types {
            ty.visit_definition_ids(
                &mut |id| {
                    owner
                        .definitions()
                        .resolve(*id)
                        .map(|_| ())
                        .map_err(DefinitionMappingError::from)
                },
                &cancel,
            )
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        }
        self.type_arguments(owner, None, types)
    }

    pub(crate) fn type_arguments(
        &self,
        owner: &ExecutableModule,
        environment: Option<Arc<TypeBindings>>,
        types: &[Ty],
    ) -> Result<Vec<TypeArgument>, RuntimeError> {
        let invalid = || RuntimeError::module_validation("type argument scope");
        // Reified types own immutable metadata, not executable module instances.
        if !owner.belongs_to(self.host.owner()) {
            return Err(invalid());
        }
        let mut scope = None;
        let definitions = self.definition_context().snapshot();
        let mut arguments = Vec::with_capacity(types.len());
        for expression in types {
            if let Ty::Parameter {
                owner: binder,
                position,
            } = expression
            {
                let argument = environment
                    .as_ref()
                    .and_then(|environment| environment.argument(binder, *position))
                    .ok_or_else(invalid)?;
                argument.validate_for(self, owner)?;
                arguments.push(argument.clone());
                continue;
            }
            let ty = TypeView::new(expression, owner, environment.as_deref())
                .closed()
                .ok_or_else(invalid)?
                .into_owned();
            if !ty.is_concrete() || !types_in_scope([&ty], &[], &Default::default(), &definitions) {
                return Err(invalid());
            }
            let origin = if contains_scoped_nominal(&ty) {
                let scope = match &scope {
                    Some(scope) => Arc::clone(scope),
                    None => {
                        let prepared = Arc::new(TypeScope {
                            owner: owner.clone(),
                            environment: environment.clone(),
                        });
                        scope = Some(prepared.clone());
                        prepared
                    }
                };
                Some(Arc::new(TypeOrigin {
                    expression: expression.clone(),
                    scope,
                }))
            } else {
                None
            };
            let mut registry = None;
            ty.visit_definition_ids(
                &mut |_| {
                    registry = Some(self.host.owner());
                    Ok(())
                },
                &Default::default(),
            )
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
            let argument = TypeArgument {
                data: Arc::new(TypeArgumentData {
                    registry,
                    ty,
                    definitions: definitions.clone(),
                    origin,
                    parameters: OnceLock::new(),
                    variants: OnceLock::new(),
                    identity: OnceLock::new(),
                    admission: OnceLock::new(),
                }),
            };
            // Prepare immutable nominal meaning at type application, before a
            // value matcher or a closed native kernel consumes this argument.
            argument.validate_for(self, owner)?;
            argument.prepare_admission(self, owner);
            arguments.push(argument);
        }
        Ok(arguments)
    }
}

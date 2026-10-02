//! Reachable concrete layouts and declaration-scoped aggregate templates share the function instantiation budget.

use crate::source::{
    lower::{MirLoweringError, instances::InstancePlanner},
    types::raise_type,
};
use kagari_abi::{
    layout::{EnumLayout, EnumVariantLayout, StructFieldLayout, StructLayout},
    types::{GenericParameterAbi, verify::types_in_scope},
};
use kagari_common::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_hir::{
    AnalyzedModule,
    types::{
        NominalType, TypeId, TypeSubstitution,
        abi::{lower_nominal_type, lower_type},
    },
};
use kagari_mir::function::MirFunction;
use std::{
    collections::{HashSet, VecDeque},
    mem, slice,
};

pub(super) fn collect(
    module: &AnalyzedModule,
    planner: &mut InstancePlanner<'_>,
    functions: &[MirFunction],
) -> Result<(Vec<StructLayout>, Vec<EnumLayout>), MirLoweringError> {
    let mut pending = VecDeque::new();
    for structure in module
        .aggregates
        .structures()
        .filter(|s| s.generic_params.is_empty())
    {
        pending.push_back((
            TypeId::Struct(NominalType {
                associated_types: Default::default(),
                declaration: structure.id.clone(),
                arguments: Vec::new(),
            }),
            structure.declaration.location.range,
        ));
    }
    for enumeration in module
        .aggregates
        .enumerations()
        .filter(|s| s.generic_params.is_empty())
    {
        pending.push_back((
            TypeId::Enum(NominalType {
                associated_types: Default::default(),
                declaration: enumeration.id.clone(),
                arguments: Vec::new(),
            }),
            enumeration.declaration.location.range,
        ));
    }
    // Synthesized adapters and closure bodies have their own closed signature.
    // Their source-context function is not their executable type contract.
    for function in functions {
        planner.check()?;
        for ty in function
            .semantic
            .params
            .values()
            .chain(function.semantic.result.iter())
        {
            pending.push_back((raise_type(ty), function.debug.source_span));
        }
    }
    // Expression roots are recorded only when lowering visits reachable code.
    // Preserve visit order so layout slots and artifact bytes are stable.
    pending.extend(mem::take(&mut planner.layout_roots));
    let mut seen = HashSet::new();
    let mut structures = Vec::new();
    let mut enumerations = Vec::new();
    let mut scope = functions
        .iter()
        .filter_map(|function| function.semantic.generic.as_ref())
        .chain(
            planner
                .native_targets
                .iter()
                .filter_map(|import| import.generic.as_ref()),
        )
        .flat_map(|body| body.parameters.iter().cloned())
        .collect::<Vec<_>>();
    while let Some((mut ty, span)) = pending.pop_front() {
        planner.check()?;
        if !ty.is_concrete() && !types_in_scope([&lower_type(&ty)], &scope, &planner.options.cancel)
        {
            return Err(MirLoweringError::diagnostic(
                Diagnostic::error(DiagnosticKind::UnresolvedConcreteType {
                    type_name: ty.display_name(),
                })
                .with_span(span),
            ));
        }
        let parameters = match &ty {
            TypeId::Struct(nominal) if !ty.is_concrete() => planner
                .aggregate_catalog(&nominal.declaration)
                .structure(&nominal.declaration)
                .map(|template| template.generic_params.clone()),
            TypeId::Enum(nominal) if !ty.is_concrete() => planner
                .aggregate_catalog(&nominal.declaration)
                .enumeration(&nominal.declaration)
                .map(|template| template.generic_params.clone()),
            _ => None,
        };
        if let Some(parameters) = parameters {
            let (TypeId::Struct(nominal) | TypeId::Enum(nominal)) = &mut ty else {
                unreachable!()
            };
            if parameters.len() != nominal.arguments.len() {
                return Err(MirLoweringError::MissingBinding(
                    "aggregate layout type arguments",
                ));
            }
            pending.extend(
                mem::take(&mut nominal.arguments)
                    .into_iter()
                    .map(|ty| (ty, span)),
            );
            for parameter in parameters {
                scope.push(GenericParameterAbi {
                    owner: parameter.owner.clone(),
                    position: parameter.position,
                });
                nominal.arguments.push(TypeId::Generic(parameter));
            }
        }
        if !seen.insert(ty.clone()) {
            continue;
        }
        match ty {
            TypeId::Host(id) => {
                planner.host_types.insert(id);
            }
            TypeId::Struct(nominal) => {
                let template = planner
                    .aggregate_catalog(&nominal.declaration)
                    .structure(&nominal.declaration)
                    .ok_or(MirLoweringError::MissingBinding("struct layout template"))?;
                if template.generic_params.len() != nominal.arguments.len() {
                    return Err(MirLoweringError::MissingBinding(
                        "struct layout type arguments",
                    ));
                }
                if !nominal.arguments.is_empty() {
                    planner.charge_layout_instance(span)?;
                }
                let substitution: TypeSubstitution = template
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(nominal.arguments.iter().cloned())
                    .collect();
                let mut fields = Vec::new();
                for field in &template.fields {
                    let ty = planner
                        .arguments(slice::from_ref(&field.ty), &substitution, span)?
                        .remove(0);
                    planner.value_type(&ty, &substitution, span)?;
                    fields.push(StructFieldLayout {
                        declaration: field.id.clone(),
                        name: field.name.clone(),
                        ty: lower_type(&ty),
                        mutable: field.writeability.is_var(),
                    });
                    pending.push_back((ty, span));
                }
                let identity = lower_nominal_type(&nominal);
                structures.push(StructLayout {
                    declaration: identity.declaration,
                    arguments: identity.arguments,
                    fields,
                });
                pending.extend(nominal.arguments.into_iter().map(|ty| (ty, span)));
            }
            TypeId::Enum(nominal) => {
                let template = planner
                    .aggregate_catalog(&nominal.declaration)
                    .enumeration(&nominal.declaration)
                    .ok_or(MirLoweringError::MissingBinding("enum layout template"))?;
                if template.generic_params.len() != nominal.arguments.len() {
                    return Err(MirLoweringError::MissingBinding(
                        "enum layout type arguments",
                    ));
                }
                if !nominal.arguments.is_empty() {
                    planner.charge_layout_instance(span)?;
                }
                let substitution: TypeSubstitution = template
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(nominal.arguments.iter().cloned())
                    .collect();
                let mut variants = Vec::new();
                for variant in &template.variants {
                    let types = planner.arguments(&variant.payload, &substitution, span)?;
                    for ty in &types {
                        planner.value_type(ty, &substitution, span)?;
                    }
                    let payload = types.iter().map(lower_type).collect();
                    variants.push(EnumVariantLayout {
                        declaration: variant.id.clone(),
                        payload,
                    });
                    pending.extend(types.into_iter().map(|ty| (ty, span)));
                }
                let identity = lower_nominal_type(&nominal);
                enumerations.push(EnumLayout {
                    declaration: identity.declaration,
                    arguments: identity.arguments,
                    variants,
                });
                pending.extend(nominal.arguments.into_iter().map(|ty| (ty, span)));
            }
            TypeId::Tuple(types) | TypeId::StandardEnum { args: types, .. } => {
                pending.extend(types.into_iter().map(|ty| (ty, span)))
            }
            TypeId::Array(ty, _) | TypeId::Set(ty, _) | TypeId::Iter(ty) | TypeId::Range(ty, _) => {
                pending.push_back((*ty, span))
            }
            TypeId::Map { key, value, .. } => {
                pending.push_back((*key, span));
                pending.push_back((*value, span));
            }
            TypeId::NativeObject(nominal) | TypeId::Trait(nominal) => {
                pending.extend(nominal.arguments.into_iter().map(|ty| (ty, span)));
                pending.extend(nominal.associated_types.into_values().map(|ty| (ty, span)));
            }
            _ => {}
        }
    }
    Ok((structures, enumerations))
}

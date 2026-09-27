//! Reachable concrete aggregate layouts share the function instantiation budget.
use crate::source::types::{lower_nominal_type, lower_type};

use crate::source::lower::MirLoweringError;
use crate::source::lower::instances::InstancePlanner;
use kagari_abi::layout::EnumLayout;
use kagari_abi::layout::EnumVariantLayout;
use kagari_abi::layout::StructFieldLayout;
use kagari_abi::layout::StructLayout;
use kagari_common::{Diagnostic, DiagnosticKind};
use kagari_hir::{
    AnalyzedModule,
    types::{NominalType, TypeId, TypeSubstitution},
};
use std::collections::{HashSet, VecDeque};
use std::mem;
use std::slice;

pub(super) fn collect(
    module: &AnalyzedModule,
    planner: &mut InstancePlanner<'_>,
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
    for instance in &planner.instances {
        planner.check()?;
        let signature = module
            .typed
            .functions
            .iter()
            .find(|f| f.id == instance.function)
            .ok_or(MirLoweringError::MissingTypedFunction(instance.function))?;
        let span = module.lowered.source_map.function_span(instance.function);
        let mut roots = signature
            .params
            .iter()
            .map(|p| p.ty.clone())
            .collect::<Vec<_>>();
        roots.push(signature.return_type.clone());
        for root in roots {
            pending.push_back((
                planner
                    .arguments(&[root], &instance.substitution, span)?
                    .remove(0),
                span,
            ));
        }
    }
    // Expression roots are recorded only when lowering visits reachable code.
    // Preserve visit order so layout slots and artifact bytes are stable.
    pending.extend(mem::take(&mut planner.layout_roots));
    let mut seen = HashSet::new();
    let mut structures = Vec::new();
    let mut enumerations = Vec::new();
    while let Some((ty, span)) = pending.pop_front() {
        planner.check()?;
        if !ty.is_concrete() {
            return Err(MirLoweringError::diagnostic(
                Diagnostic::error(DiagnosticKind::UnresolvedConcreteType {
                    type_name: ty.display_name(),
                })
                .with_span(span),
            ));
        }
        if !seen.insert(ty.clone()) {
            continue;
        }
        match ty {
            TypeId::Host(id) => {
                planner.host_types.insert(id);
            }
            TypeId::Struct(nominal) => {
                let template = module
                    .aggregates
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
                    planner.value_type(&ty, &TypeSubstitution::default(), span)?;
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
                let template = module
                    .aggregates
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
                        planner.value_type(ty, &TypeSubstitution::default(), span)?;
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
            TypeId::Trait(nominal) => {
                pending.extend(nominal.arguments.into_iter().map(|ty| (ty, span)));
                pending.extend(nominal.associated_types.into_values().map(|ty| (ty, span)));
            }
            _ => {}
        }
    }
    Ok((structures, enumerations))
}

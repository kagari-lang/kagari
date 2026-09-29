use crate::{
    builtin::traits,
    declarations::Declarations,
    hir::{FunctionId, TypeKind},
    lower::LoweredModule,
    resolver::ResolvedName,
    typeck::{
        ConstraintTarget, FunctionTypeIndex, TypeTable,
        check::{
            interface_method_compatible, possibly_overlapping_impls,
            validate_standard_constraint_type,
        },
        constraints,
        ty::{display_type, display_type_id},
    },
    types::{NominalType, TypeId},
};
use kagari_abi::standard::traits::StandardTrait;
use kagari_common::{Diagnostic, DiagnosticKind, Span, identity};
use smallvec::SmallVec;
use std::iter;
pub(super) fn validate_trait_surface(
    lowered: &LoweredModule,
    declarations: &Declarations,
    function_index: &FunctionTypeIndex,
    table: &mut TypeTable,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    for function in &lowered.module.functions {
        let Some(typed_function) = function_index.by_id.get(&function.id) else {
            continue;
        };
        for ty in typed_function
            .params
            .iter()
            .map(|param| &param.ty)
            .chain(iter::once(&typed_function.return_type))
        {
            validate_interface_type(
                lowered,
                declarations,
                function_index,
                ty,
                lowered.source_map.function_span(function.id),
                diagnostics,
            );
        }
    }

    let mut seen_impls: Vec<(NominalType, TypeId)> = Vec::new();
    for impl_block in &lowered.module.impls {
        let Some(reference) = &impl_block.trait_ref else {
            continue;
        };
        let trait_name = display_type(&lowered.module, reference.ty);
        let Some(target) = table.constraint(reference.ty) else {
            continue;
        };
        let ConstraintTarget::Trait(id) = target else {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitReference {
                    trait_name: trait_name.clone(),
                    reason: "standard constraints cannot be implemented by a trait impl",
                })
                .with_span(lowered.source_map.type_span(reference.ty)),
            );
            continue;
        };
        let trait_def = declarations
            .definition_target(&id.declaration)
            .and_then(|target| match target {
                ResolvedName::Trait(local_id) => lowered
                    .module
                    .traits
                    .iter()
                    .find(|item| item.id == local_id),
                _ => None,
            });
        let imported_trait = declarations
            .imported_types()
            .by_declaration(&id.declaration);

        if let (Some(trait_def), Some(TypeId::Trait(applied))) = (
            trait_def,
            table.type_ref(reference.ty).map(|entry| &entry.ty),
        ) {
            let required =
                constraints::parameter_bounds(&trait_def.generic_params, declarations, table);
            let available = constraints::implementation_bounds(impl_block, declarations, table);
            let source_arguments = match &lowered.module.type_ref(reference.ty).kind {
                TypeKind::Generic { args, .. } => args.as_slice(),
                _ => &[],
            };
            for ((parameter, actual), source_argument) in trait_def
                .generic_params
                .iter()
                .zip(&applied.arguments)
                .zip(source_arguments)
            {
                if actual.is_unresolved() {
                    continue;
                }
                let Some(parameter) = declarations.generic_type(parameter.id) else {
                    continue;
                };
                let span = lowered.source_map.type_span(*source_argument);
                for constraint in required
                    .get(&TypeId::Generic(parameter))
                    .into_iter()
                    .flatten()
                {
                    match constraint {
                        ConstraintTarget::Standard(standard) => {
                            validate_standard_constraint_type(
                                actual,
                                *standard,
                                &available,
                                span,
                                diagnostics,
                            );
                        }
                        ConstraintTarget::Trait(required_trait) => {
                            let satisfied = StandardTrait::from_id(&required_trait.declaration)
                                .is_some_and(|kind| {
                                    required_trait.arguments.is_empty()
                                        && required_trait.associated_types.is_empty()
                                        && traits::intrinsic_holds(kind, actual, None, &available)
                                })
                                || match actual {
                                    TypeId::Generic(parameter) => available
                                        .get(&TypeId::Generic(parameter.clone()))
                                        .is_some_and(|bounds| bounds.contains(constraint)),
                                    _ => table.implements(required_trait, actual),
                                };
                            if !satisfied {
                                diagnostics.push(
                                    Diagnostic::error(DiagnosticKind::GenericBoundNotSatisfied {
                                        type_name: actual.display_name(),
                                        trait_name: required_trait
                                            .declaration
                                            .path
                                            .last()
                                            .map(|part| part.name.clone())
                                            .unwrap_or_default(),
                                    })
                                    .with_span(span),
                                );
                            }
                        }
                    }
                }
            }
        }

        let Some(for_ty) = impl_block
            .for_type
            .and_then(|ty| table.type_ref(ty))
            .map(|resolved| resolved.ty.clone())
            .filter(|ty| !ty.is_unresolved())
        else {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                    trait_name: trait_name.to_string(),
                    type_name: "<missing>".to_string(),
                    reason: "impl target type is unknown".to_string(),
                })
                .with_span(lowered.source_map.impl_span(impl_block.id)),
            );
            continue;
        };
        let type_name = display_type_id(&for_ty);
        if matches!(for_ty, TypeId::Trait(_) | TypeId::Generic(_)) {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                    trait_name: trait_name.to_string(),
                    type_name,
                    reason: "impl target must be a concrete type".to_string(),
                })
                .with_span(lowered.source_map.impl_span(impl_block.id)),
            );
            continue;
        }

        if seen_impls.iter().any(|(previous_trait, previous_type)| {
            previous_trait.declaration == id.declaration
                && (previous_trait.arguments == id.arguments
                    || !previous_trait.arguments.iter().all(TypeId::is_concrete)
                    || !id.arguments.iter().all(TypeId::is_concrete))
                && possibly_overlapping_impls(previous_type, &for_ty)
        }) {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                    trait_name: trait_name.to_string(),
                    type_name: type_name.clone(),
                    reason: "overlapping impl".to_string(),
                })
                .with_span(lowered.source_map.impl_span(impl_block.id)),
            );
            continue;
        }
        seen_impls.push((id.clone(), for_ty.clone()));

        let standard = StandardTrait::from_id(&id.declaration);
        if standard.is_some_and(|kind| {
            !kind.host_implementable() && matches!(for_ty, TypeId::Host(_))
                || lowered.installed_stdlib.is_none()
                    && !kind.conversion()
                    && !matches!(
                        for_ty,
                        TypeId::Struct(_) | TypeId::Enum(_) | TypeId::Host(_)
                    )
        }) {
            diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidTraitImpl { trait_name: trait_name.clone(), type_name: type_name.clone(), reason: "standard operator/equality impls require a script Struct or enum; formatting requires a nominal receiver".into() }).with_span(lowered.source_map.impl_span(impl_block.id)));
            continue;
        }
        let methods = if let Some(trait_def) = trait_def {
            trait_def
                .methods
                .iter()
                .filter_map(|method| {
                    impl_block
                        .methods
                        .iter()
                        .find(|implementation| implementation.name == method.name)
                        .map(|implementation| {
                            (
                                declarations
                                    .definition(ResolvedName::Function(method.function))
                                    .expect("trait method declaration")
                                    .clone(),
                                implementation.function,
                            )
                        })
                })
                .collect()
        } else {
            imported_trait
                .into_iter()
                .flat_map(|trait_type| &trait_type.trait_methods)
                .filter_map(|method| {
                    impl_block
                        .methods
                        .iter()
                        .find(|implementation| implementation.name == method.name)
                        .map(|implementation| (method.declaration.clone(), implementation.function))
                })
                .collect()
        };
        let parameters = impl_block
            .generic_params
            .iter()
            .filter_map(|parameter| declarations.generic_type(parameter.id))
            .collect();
        let bounds = constraints::implementation_bounds(impl_block, declarations, table);
        table.insert_implementation(
            declarations
                .impl_identity(impl_block.id)
                .expect("checked impl declaration identity")
                .clone(),
            id,
            for_ty,
            parameters,
            bounds,
            methods,
        );
    }
}

pub(super) fn trait_method_interface_compatible(
    lowered: &LoweredModule,
    function_index: &FunctionTypeIndex,
    function_id: FunctionId,
    trait_generic_count: usize,
    interface: &NominalType,
) -> bool {
    let Some(hir_function) = lowered
        .module
        .functions
        .iter()
        .find(|function| function.id == function_id)
    else {
        return false;
    };
    let Some(function) = function_index.by_id.get(&function_id) else {
        return false;
    };
    interface_method_compatible(
        hir_function.generic_params.len(),
        trait_generic_count,
        function
            .params
            .first()
            .is_some_and(|param| param.name == "self"),
        &function.return_type.with_associated_types(interface),
        function
            .params
            .iter()
            .skip(1)
            .map(|param| param.ty.with_associated_types(interface))
            .collect::<Vec<_>>()
            .iter(),
    )
}

pub(super) fn validate_interface_type(
    lowered: &LoweredModule,
    declarations: &Declarations,
    function_index: &FunctionTypeIndex,
    ty: &TypeId,
    span: Span,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    if let TypeId::Struct(nominal) | TypeId::Enum(nominal) | TypeId::Trait(nominal) = ty {
        for argument in &nominal.arguments {
            validate_interface_type(
                lowered,
                declarations,
                function_index,
                argument,
                span,
                diagnostics,
            );
        }
    }
    match ty {
        TypeId::Trait(trait_name) => {
            if let Some(trait_def) = lowered.module.traits.iter().find(|trait_def| {
                declarations.definition(ResolvedName::Trait(trait_def.id))
                    == Some(&trait_name.declaration)
            }) {
                if !trait_def.associated_consts.is_empty() {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidInterfaceType {
                            trait_name: trait_def.name.clone(),
                            reason: "traits with associated constants only support static dispatch"
                                .into(),
                        })
                        .with_span(span),
                    );
                }
                if trait_def
                    .associated_types
                    .iter()
                    .any(|member| !member.generic_params.is_empty())
                {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidInterfaceType {
                            trait_name: trait_def.name.clone(),
                            reason:
                                "traits with generic associated types only support static dispatch"
                                    .into(),
                        })
                        .with_span(span),
                    );
                }
                if trait_def.associated_types.iter().any(|member| {
                    !trait_name
                        .associated_types
                        .contains_key(&identity::associated_type_id(
                            &trait_name.declaration,
                            &member.name,
                        ))
                }) {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidInterfaceType {
                            trait_name: trait_def.name.clone(),
                            reason: "interface values require bindings for all associated types"
                                .into(),
                        })
                        .with_span(span),
                    );
                }
                for method in &trait_def.methods {
                    if !trait_method_interface_compatible(
                        lowered,
                        function_index,
                        method.function,
                        trait_def.generic_params.len(),
                        trait_name,
                    ) {
                        diagnostics.push(
                            Diagnostic::error(DiagnosticKind::InvalidInterfaceType {
                                trait_name: trait_def.name.clone(),
                                reason: format!(
                                    "method `{}` is not interface-compatible",
                                    method.name
                                ),
                            })
                            .with_span(span),
                        );
                    }
                }
            }
        }
        TypeId::Tuple(elements) => {
            for element in elements {
                validate_interface_type(
                    lowered,
                    declarations,
                    function_index,
                    element,
                    span,
                    diagnostics,
                );
            }
        }
        TypeId::Function { params, result } => {
            for parameter in params {
                validate_interface_type(
                    lowered,
                    declarations,
                    function_index,
                    parameter,
                    span,
                    diagnostics,
                );
            }
            validate_interface_type(
                lowered,
                declarations,
                function_index,
                result,
                span,
                diagnostics,
            );
        }
        TypeId::Array(element, _) | TypeId::Iter(element) | TypeId::Range(element, _) => {
            validate_interface_type(
                lowered,
                declarations,
                function_index,
                element,
                span,
                diagnostics,
            );
        }
        TypeId::Map { key, value, .. } => {
            validate_interface_type(
                lowered,
                declarations,
                function_index,
                key,
                span,
                diagnostics,
            );
            validate_interface_type(
                lowered,
                declarations,
                function_index,
                value,
                span,
                diagnostics,
            );
        }
        TypeId::Set(element, _) => {
            validate_interface_type(
                lowered,
                declarations,
                function_index,
                element,
                span,
                diagnostics,
            );
        }
        TypeId::StandardEnum { args, .. } => {
            for arg in args {
                validate_interface_type(
                    lowered,
                    declarations,
                    function_index,
                    arg,
                    span,
                    diagnostics,
                );
            }
        }
        _ => {}
    }
}

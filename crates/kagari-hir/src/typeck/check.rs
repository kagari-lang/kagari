use kagari_common::{Diagnostic, DiagnosticKind, TypePosition};
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};

use crate::{
    AnalysisResult,
    builtin::surface::{self, StandardTypeConstraint},
    hir::FunctionKind,
    hir::{BinaryOp, ConstId, ExprId, ExprKind, PrefixOp},
    lower::LoweredModule,
    resolver::{ResolvedName, ResolvedNames},
    typeck::body::BodyChecker,
    typeck::ty::{TypeContext, display_type, display_type_id, resolve_type, resolve_type_in},
    typeck::{
        BodyTypeEnv, FunctionTypeIndex, TopLevelTypeIndex, TypeIndexes, TypeTable, TypedFunction,
        TypedFunctionBuffer, TypedModule, TypedParameter, TypedParameterBuffer,
    },
    types::{BuiltinType, TypeId},
};

pub(crate) fn check_signatures(
    lowered: &LoweredModule,
    declarations: &crate::declarations::Declarations,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> AnalysisResult<super::ModuleSignatures> {
    let mut diagnostics = SmallVec::<[Diagnostic; 4]>::new();
    let mut functions: TypedFunctionBuffer = SmallVec::new();
    let mut function_index = FunctionTypeIndex::default();
    let mut type_table = TypeTable::default();
    super::constraints::resolve_constraints(
        lowered,
        declarations,
        &mut type_table,
        &mut diagnostics,
        cancel,
    );

    let mut type_bounds = HashMap::new();
    for (target, params) in lowered
        .module
        .structs
        .iter()
        .map(|s| (ResolvedName::Struct(s.id), &s.generic_params))
        .chain(
            lowered
                .module
                .enums
                .iter()
                .map(|e| (ResolvedName::Enum(e.id), &e.generic_params)),
        )
    {
        if cancel.check().is_err() {
            break;
        }
        let id = declarations
            .definition(target)
            .expect("type declaration")
            .clone();
        type_bounds.insert(
            id,
            super::constraints::parameter_bounds(params, declarations, &type_table),
        );
    }

    for structure in &lowered.module.structs {
        let mut field_names = HashSet::new();
        for field in &structure.fields {
            if cancel.check().is_err() {
                break;
            }
            if !field_names.insert(&field.name) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::DuplicateField {
                        struct_name: structure.name.clone(),
                        name: field.name.clone(),
                    })
                    .with_span(lowered.source_map.field_span(field.id)),
                );
            }
            let ty = resolve_type_in(
                &lowered.module,
                field.ty,
                TypeContext {
                    declarations,
                    generics: &structure.generic_params,
                    self_type: None,
                },
                &mut type_table,
                cancel,
            );
            if ty.is_unresolved() {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                        type_name: display_type(&lowered.module, field.ty),
                    })
                    .with_span(lowered.source_map.type_span(field.ty)),
                );
            }
            {
                let ty = &ty;
                let id = declarations
                    .definition(ResolvedName::Struct(structure.id))
                    .expect("struct declaration");
                validate_standard_type_constraints(
                    ty,
                    &type_bounds[id],
                    lowered.source_map.type_span(field.ty),
                    &mut diagnostics,
                    cancel,
                );
            }
            type_table.insert_field_type(field.id, ty);
        }
    }

    for enumeration in &lowered.module.enums {
        for variant in &enumeration.variants {
            for payload in &variant.payload {
                if cancel.check().is_err() {
                    break;
                }
                match resolve_type_in(
                    &lowered.module,
                    *payload,
                    TypeContext {
                        declarations,
                        generics: &enumeration.generic_params,
                        self_type: None,
                    },
                    &mut type_table,
                    cancel,
                ) {
                    ty if !ty.is_unresolved() => validate_standard_type_constraints(
                        &ty,
                        &type_bounds[declarations
                            .definition(ResolvedName::Enum(enumeration.id))
                            .expect("enum declaration")],
                        lowered.source_map.type_span(*payload),
                        &mut diagnostics,
                        cancel,
                    ),
                    ty => {
                        validate_standard_type_constraints(
                            &ty,
                            &type_bounds[declarations
                                .definition(ResolvedName::Enum(enumeration.id))
                                .expect("enum declaration")],
                            lowered.source_map.type_span(*payload),
                            &mut diagnostics,
                            cancel,
                        );
                        diagnostics.push(
                            Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                                type_name: display_type(&lowered.module, *payload),
                            })
                            .with_span(lowered.source_map.type_span(*payload)),
                        );
                    }
                }
            }
        }
    }

    for implementation in &lowered.module.impls {
        if let Some(ty) = implementation.for_type {
            let resolved = resolve_type_in(
                &lowered.module,
                ty,
                TypeContext {
                    declarations,
                    generics: &implementation.generic_params,
                    self_type: None,
                },
                &mut type_table,
                cancel,
            );
            if resolved.is_unresolved()
                && implementation.trait_ref.is_none()
                && cancel.check().is_ok()
            {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                        type_name: display_type(&lowered.module, ty),
                    })
                    .with_span(lowered.source_map.type_span(ty)),
                );
            }
        }
    }

    for function in &lowered.module.functions {
        if cancel.check().is_err() {
            break;
        }
        if function.visibility != crate::hir::Visibility::Private
            && !function.generic_params.is_empty()
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::PublicGenericFunction {
                    name: function.name.clone(),
                })
                .with_span(lowered.source_map.function_span(function.id)),
            );
        }
        let mut params: TypedParameterBuffer = SmallVec::new();
        let context = function_type_context(&lowered.module, function, declarations);
        let bounds = super::constraints::function_bounds(
            &lowered.module,
            function,
            declarations,
            &type_table,
        );
        let function_name = if function.name.is_empty() {
            "<missing>".to_string()
        } else {
            function.name.clone()
        };

        for param in &function.params {
            let param_name = if param.name.is_empty() {
                "<missing>".to_string()
            } else {
                param.name.clone()
            };
            let param_ty_name = display_type(&lowered.module, param.ty);

            // An implicit impl receiver reuses the impl header's type reference.
            // Its generics belong to the impl, even if this method shadows a name.
            let param_type = if function.kind == FunctionKind::ImplMethod && param.name == "self" {
                type_table
                    .type_ref(param.ty)
                    .map(|resolved| resolved.ty.clone())
                    .unwrap_or(TypeId::Error)
            } else {
                resolve_type_in(&lowered.module, param.ty, context, &mut type_table, cancel)
            };
            validate_standard_type_constraints(
                &param_type,
                &bounds,
                lowered.source_map.type_span(param.ty),
                &mut diagnostics,
                cancel,
            );
            if param_type.is_unresolved() {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownType {
                        type_name: param_ty_name,
                        function_name: function_name.clone(),
                        position: TypePosition::Parameter,
                    })
                    .with_span(lowered.source_map.type_span(param.ty)),
                );
            }
            params.push(TypedParameter {
                id: param.id,
                writeability: param.writeability,
                name: param_name,
                ty: param_type,
            });
        }

        let return_type = match &function.return_type {
            Some(ty_ref) => {
                let ty =
                    resolve_type_in(&lowered.module, *ty_ref, context, &mut type_table, cancel);
                validate_standard_type_constraints(
                    &ty,
                    &bounds,
                    lowered.source_map.type_span(*ty_ref),
                    &mut diagnostics,
                    cancel,
                );
                if ty.is_unresolved() {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnknownType {
                            type_name: display_type(&lowered.module, *ty_ref),
                            function_name: function_name.clone(),
                            position: TypePosition::Return,
                        })
                        .with_span(lowered.source_map.type_span(*ty_ref)),
                    );
                }
                ty
            }
            None => TypeId::Builtin(BuiltinType::Unit),
        };

        let typed_function = TypedFunction {
            bounds,
            generic_params: function
                .generic_params
                .iter()
                .filter_map(|parameter| declarations.generic_type(parameter.id))
                .collect(),
            id: function.id,
            name: function_name,
            params,
            return_type,
        };
        function_index
            .by_id
            .insert(function.id, typed_function.clone());
        functions.push(typed_function);
    }

    validate_trait_surface(
        lowered,
        declarations,
        &function_index,
        &mut type_table,
        &mut diagnostics,
    );
    AnalysisResult {
        facts: super::ModuleSignatures {
            type_bounds,
            functions,
            type_table,
        },
        diagnostics,
    }
}

pub(crate) fn check_bodies_controlled(
    lowered: &LoweredModule,
    names: &ResolvedNames,
    declarations: &crate::declarations::Declarations,
    inputs: super::BodyInputs<'_>,
    reuse: Option<&super::BodyReuse<'_>>,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> AnalysisResult<TypedModule> {
    let super::BodyInputs {
        const_limits,
        selection,
        signatures,
        imported_functions,
        aggregates,
    } = inputs;
    let reuse = reuse.filter(|reuse| reuse.environment_matches(lowered));
    let mut checked_bodies = 0;
    let mut reused_bodies = 0;
    let mut diagnostics = if matches!(selection, crate::hir::BodySelection::All) {
        signatures.diagnostics.clone()
    } else {
        Default::default()
    };
    let functions = signatures.facts.functions.clone();
    let function_index = FunctionTypeIndex {
        by_id: functions.iter().map(|f| (f.id, f.clone())).collect(),
    };
    let mut top_level_index = TopLevelTypeIndex::default();
    let mut type_table = signatures.facts.type_table.clone();
    {
        // Explicit constant declarations are visible throughout the module,
        // independently of initializer evaluation order.
        for const_item in &lowered.module.consts {
            if cancel.check().is_err() {
                break;
            }
            let Some(ty_ref) = const_item.ty else {
                continue;
            };
            let ty = match resolve_type(
                &lowered.module,
                ty_ref,
                declarations,
                &mut type_table,
                cancel,
            ) {
                ty if !ty.is_unresolved() => {
                    validate_standard_type_constraints(
                        &ty,
                        &HashMap::new(),
                        lowered.source_map.type_span(ty_ref),
                        &mut diagnostics,
                        cancel,
                    );
                    ty
                }
                ty => {
                    validate_standard_type_constraints(
                        &ty,
                        &HashMap::new(),
                        lowered.source_map.type_span(ty_ref),
                        &mut diagnostics,
                        cancel,
                    );
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnknownConstType {
                            const_name: const_item.name.clone(),
                            type_name: display_type(&lowered.module, ty_ref),
                        })
                        .with_span(lowered.source_map.const_span(const_item.id)),
                    );
                    ty
                }
            };
            top_level_index.consts.insert(const_item.id, ty);
        }
        for const_item in &lowered.module.consts {
            if cancel.check().is_err() {
                break;
            }
            let ty = if let Some(ty) = top_level_index.consts.get(&const_item.id) {
                ty.clone()
            } else {
                let mut env = BodyTypeEnv::default();
                let mut checker = BodyChecker::new(
                    lowered,
                    names,
                    TypeIndexes {
                        aggregates,
                        imported_functions,
                        declarations,
                        cancel,
                        function_index: &function_index,
                        top_level_index: &top_level_index,
                        const_values: None,
                    },
                    &mut diagnostics,
                    &mut type_table,
                    "<const>",
                    TypeId::Builtin(BuiltinType::Unit),
                );
                checker.infer_expr_type(const_item.initializer, &mut env)
            };
            if const_item.ty.is_some() {
                let mut env = BodyTypeEnv::default();
                let mut checker = BodyChecker::new(
                    lowered,
                    names,
                    TypeIndexes {
                        aggregates,
                        imported_functions,
                        declarations,
                        cancel,
                        function_index: &function_index,
                        top_level_index: &top_level_index,
                        const_values: None,
                    },
                    &mut diagnostics,
                    &mut type_table,
                    "<const>",
                    TypeId::Builtin(BuiltinType::Unit),
                );
                let _ = checker.infer_expr_type(const_item.initializer, &mut env);
            }
            top_level_index.consts.insert(const_item.id, ty.clone());
        }

        let mut const_budget = super::const_budget::ConstBudget::new(const_limits);
        validate_const_initializers(
            lowered,
            names,
            &top_level_index,
            &type_table,
            cancel,
            &mut diagnostics,
            &mut const_budget,
        );

        let const_values = super::const_eval::evaluate_constants(
            lowered,
            names,
            &type_table,
            cancel,
            &mut diagnostics,
            &mut const_budget,
        );

        for function in &lowered.module.functions {
            if cancel.check().is_err() {
                break;
            }
            if matches!(function.kind, FunctionKind::TraitMethod) {
                continue;
            }
            if !selection.includes(function.id) {
                continue;
            }
            if reuse.is_some_and(|reuse| reuse.restore(lowered, function, &mut type_table)) {
                reused_bodies += 1;
                continue;
            }
            checked_bodies += 1;
            let mut env = BodyTypeEnv::default();
            if let Some(typed_function) = function_index.by_id.get(&function.id) {
                env.generics = function.generic_params.clone();
                env.generic_bounds = typed_function.bounds.clone();
                for param in &typed_function.params {
                    env.params.insert(param.id, param.ty.clone());
                }
                let mut checker = BodyChecker::new(
                    lowered,
                    names,
                    TypeIndexes {
                        aggregates,
                        imported_functions,
                        declarations,
                        cancel,
                        function_index: &function_index,
                        top_level_index: &top_level_index,
                        const_values: Some(&const_values),
                    },
                    &mut diagnostics,
                    &mut type_table,
                    &typed_function.name,
                    typed_function.return_type.clone(),
                );
                let body_ty = checker.infer_block_types_expected(
                    function.body,
                    &mut env,
                    Some(&typed_function.return_type),
                );
                let Ok(completes) =
                    super::completion::block_can_complete(&lowered.module, function.body, cancel)
                else {
                    break;
                };
                if completes && body_ty.conflicts_with(&typed_function.return_type) {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ReturnTypeMismatch {
                            function_name: typed_function.name.clone(),
                            expected: display_type_id(&typed_function.return_type),
                            found: display_type_id(&body_ty),
                        })
                        .with_span(lowered.source_map.function_span(function.id)),
                    );
                }
            }
        }

        AnalysisResult {
            facts: TypedModule {
                checked_bodies,
                reused_bodies,
                functions,
                consts: top_level_index.consts,
                const_values,
                type_table,
            },
            diagnostics,
        }
    }
}

fn function_type_context<'a>(
    module: &'a crate::hir::Module,
    function: &'a crate::hir::Function,
    declarations: &'a crate::declarations::Declarations,
) -> TypeContext<'a> {
    TypeContext {
        declarations,
        generics: &function.generic_params,
        self_type: module
            .traits
            .iter()
            .find(|item| {
                item.methods
                    .iter()
                    .any(|method| method.function == function.id)
            })
            .map(|item| item.id),
    }
}
fn validate_trait_surface(
    lowered: &LoweredModule,
    declarations: &crate::declarations::Declarations,
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
            .chain(std::iter::once(&typed_function.return_type))
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

    let mut seen_impls: Vec<(crate::types::NominalType, TypeId)> = Vec::new();
    for impl_block in &lowered.module.impls {
        let Some(reference) = &impl_block.trait_ref else {
            continue;
        };
        let trait_name = display_type(&lowered.module, reference.ty);
        let Some(target) = table.constraint(reference.ty) else {
            continue;
        };
        let super::ConstraintTarget::Trait(id) = target else {
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
            let required = super::constraints::parameter_bounds(
                &trait_def.generic_params,
                declarations,
                table,
            );
            let available =
                super::constraints::implementation_bounds(impl_block, declarations, table);
            let source_arguments = match &lowered.module.type_ref(reference.ty).kind {
                crate::hir::TypeKind::Generic { args, .. } => args.as_slice(),
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
                for constraint in required.get(&parameter).into_iter().flatten() {
                    match constraint {
                        super::ConstraintTarget::Standard(standard) => {
                            validate_standard_constraint_type(
                                actual,
                                *standard,
                                &available,
                                span,
                                diagnostics,
                            );
                        }
                        super::ConstraintTarget::Trait(required_trait) => {
                            let satisfied = match actual {
                                TypeId::Generic(parameter) => available
                                    .get(parameter)
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

        if let Some(trait_def) = trait_def {
            validate_impl_methods(
                lowered,
                function_index,
                trait_def,
                impl_block,
                (
                    &for_ty,
                    declarations
                        .definition(ResolvedName::Trait(trait_def.id))
                        .expect("checked trait declaration"),
                    match table.type_ref(reference.ty).map(|resolved| &resolved.ty) {
                        Some(TypeId::Trait(ty)) => ty.arguments.as_slice(),
                        _ => &[],
                    },
                ),
                diagnostics,
            );
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
        let bounds = super::constraints::implementation_bounds(impl_block, declarations, table);
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

pub(crate) fn possibly_overlapping_impls(left: &TypeId, right: &TypeId) -> bool {
    if left == right {
        return true;
    }
    if left.is_concrete() && right.is_concrete() {
        return false;
    }
    match (left, right) {
        (TypeId::Struct(left), TypeId::Struct(right))
        | (TypeId::Enum(left), TypeId::Enum(right)) => left.declaration == right.declaration,
        (TypeId::StandardEnum { kind: left, .. }, TypeId::StandardEnum { kind: right, .. }) => {
            left == right
        }
        (TypeId::Tuple(left), TypeId::Tuple(right)) => left.len() == right.len(),
        (TypeId::Function { params: left, .. }, TypeId::Function { params: right, .. }) => {
            left.len() == right.len()
        }
        (TypeId::Array(_), TypeId::Array(_))
        | (TypeId::Set(_), TypeId::Set(_))
        | (TypeId::Map { .. }, TypeId::Map { .. }) => true,
        _ => false,
    }
}

pub(super) fn validate_standard_type_constraints(
    ty: &TypeId,
    generic_bounds: &HashMap<crate::types::GenericParameterType, Vec<super::ConstraintTarget>>,
    span: kagari_common::Span,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &kagari_common::cancellation::CancellationToken,
) {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if cancel.check().is_err() {
            return;
        }
        match ty {
            TypeId::Map { key, value } => {
                validate_standard_constraint_type(
                    key,
                    StandardTypeConstraint::HashKey,
                    generic_bounds,
                    span,
                    diagnostics,
                );
                pending.push(value);
                pending.push(key);
            }
            TypeId::Set(element) => {
                validate_standard_constraint_type(
                    element,
                    StandardTypeConstraint::HashKey,
                    generic_bounds,
                    span,
                    diagnostics,
                );
                pending.push(element);
            }
            TypeId::Tuple(elements) => pending.extend(elements.iter().rev()),
            TypeId::Function { params, result } => {
                pending.push(result);
                pending.extend(params.iter().rev());
            }
            TypeId::Array(element) => pending.push(element),
            TypeId::StandardEnum { args, .. }
            | TypeId::Struct(crate::types::NominalType {
                arguments: args, ..
            })
            | TypeId::Enum(crate::types::NominalType {
                arguments: args, ..
            })
            | TypeId::Trait(crate::types::NominalType {
                arguments: args, ..
            }) => pending.extend(args.iter().rev()),
            _ => {}
        }
    }
}

pub(super) fn validate_standard_constraint_type(
    ty: &TypeId,
    constraint: StandardTypeConstraint,
    generic_bounds: &HashMap<crate::types::GenericParameterType, Vec<super::ConstraintTarget>>,
    span: kagari_common::Span,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    if matches!(ty, TypeId::Unknown | TypeId::Error) {
        return;
    }
    if constraint == StandardTypeConstraint::Comparable && ty.is_unresolved() {
        return;
    }
    if super::constraints::type_satisfies_standard_constraint(ty, constraint, generic_bounds) {
        return;
    }

    diagnostics.push(
        Diagnostic::error(DiagnosticKind::StandardConstraintNotSatisfied {
            type_name: display_type_id(ty),
            constraint: surface::standard_constraint_name(constraint).to_owned(),
            reason: super::constraints::standard_constraint_reason(constraint).to_owned(),
        })
        .with_span(span),
    );
}

fn trait_method_interface_compatible(
    lowered: &LoweredModule,
    function_index: &FunctionTypeIndex,
    function_id: crate::hir::FunctionId,
    trait_generic_count: usize,
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
        &function.return_type,
        function.params.iter().skip(1).map(|param| &param.ty),
    )
}

pub(super) fn interface_method_compatible<'a>(
    generic_count: usize,
    trait_generic_count: usize,
    has_receiver: bool,
    return_type: &TypeId,
    other_parameters: impl IntoIterator<Item = &'a TypeId>,
) -> bool {
    generic_count == trait_generic_count
        && has_receiver
        && !return_type.contains_self_type()
        && other_parameters
            .into_iter()
            .all(|parameter| !parameter.contains_self_type())
}

fn validate_interface_type(
    lowered: &LoweredModule,
    declarations: &crate::declarations::Declarations,
    function_index: &FunctionTypeIndex,
    ty: &TypeId,
    span: kagari_common::Span,
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
                for method in &trait_def.methods {
                    if !trait_method_interface_compatible(
                        lowered,
                        function_index,
                        method.function,
                        trait_def.generic_params.len(),
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
        TypeId::Array(element) => {
            validate_interface_type(
                lowered,
                declarations,
                function_index,
                element,
                span,
                diagnostics,
            );
        }
        TypeId::Map { key, value } => {
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
        TypeId::Set(element) => {
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

fn validate_impl_methods(
    lowered: &LoweredModule,
    function_index: &FunctionTypeIndex,
    trait_def: &crate::hir::TraitDef,
    impl_block: &crate::hir::Impl,
    receiver: (&TypeId, &kagari_common::identity::DefinitionId, &[TypeId]),
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    for trait_method in &trait_def.methods {
        let Some(impl_method) = impl_block
            .methods
            .iter()
            .find(|method| method.name == trait_method.name)
        else {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::TraitMethodMismatch {
                    trait_name: trait_def.name.clone(),
                    method_name: trait_method.name.clone(),
                    reason: "missing impl method".to_string(),
                })
                .with_span(lowered.source_map.impl_span(impl_block.id)),
            );
            continue;
        };
        compare_impl_method_signature(
            function_index,
            trait_def,
            trait_method,
            impl_method,
            (
                receiver.0,
                receiver.1,
                receiver.2,
                impl_block.generic_params.len(),
            ),
            lowered.source_map.impl_span(impl_block.id),
            diagnostics,
        );
    }
    for impl_method in &impl_block.methods {
        if !trait_def
            .methods
            .iter()
            .any(|trait_method| trait_method.name == impl_method.name)
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::TraitMethodMismatch {
                    trait_name: trait_def.name.clone(),
                    method_name: impl_method.name.clone(),
                    reason: "method is not declared by trait".to_string(),
                })
                .with_span(lowered.source_map.impl_span(impl_block.id)),
            );
        }
    }
}

pub(super) trait MethodSignatureView {
    fn generic_params(&self) -> &[crate::types::GenericParameterType];
    fn bounds(&self) -> &super::GenericBounds;
    fn params_len(&self) -> usize;
    fn param(&self, index: usize) -> (&str, crate::hir::Writeability, &TypeId);
    fn return_type(&self) -> &TypeId;
}

impl MethodSignatureView for TypedFunction {
    fn generic_params(&self) -> &[crate::types::GenericParameterType] {
        &self.generic_params
    }
    fn bounds(&self) -> &super::GenericBounds {
        &self.bounds
    }
    fn params_len(&self) -> usize {
        self.params.len()
    }
    fn param(&self, index: usize) -> (&str, crate::hir::Writeability, &TypeId) {
        let param = &self.params[index];
        (&param.name, param.writeability, &param.ty)
    }
    fn return_type(&self) -> &TypeId {
        &self.return_type
    }
}

impl MethodSignatureView for crate::aggregates::MethodSignature {
    fn generic_params(&self) -> &[crate::types::GenericParameterType] {
        &self.generic_params
    }
    fn bounds(&self) -> &super::GenericBounds {
        &self.bounds
    }
    fn params_len(&self) -> usize {
        self.params.len()
    }
    fn param(&self, index: usize) -> (&str, crate::hir::Writeability, &TypeId) {
        let param = &self.params[index];
        (&param.name, param.writeability, &param.ty)
    }
    fn return_type(&self) -> &TypeId {
        &self.return_type
    }
}

pub(super) struct MethodComparison<'a> {
    pub trait_name: &'a str,
    pub method_name: &'a str,
    pub trait_generic_count: usize,
    pub impl_generic_count: usize,
    pub receiver: &'a TypeId,
    pub trait_owner: &'a kagari_common::identity::DefinitionId,
    pub trait_arguments: &'a [TypeId],
    pub span: kagari_common::Span,
}

pub(super) fn compare_method_contract(
    expected: &impl MethodSignatureView,
    actual: &TypedFunction,
    comparison: &MethodComparison<'_>,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    let mismatch = |reason: String| {
        Diagnostic::error(DiagnosticKind::TraitMethodMismatch {
            trait_name: comparison.trait_name.to_string(),
            method_name: comparison.method_name.to_string(),
            reason,
        })
        .with_span(comparison.span)
    };
    if expected.params_len() != actual.params.len() {
        diagnostics.push(mismatch("parameter count differs".into()));
        return;
    }
    let expected_method_params = expected
        .generic_params()
        .iter()
        .skip(comparison.trait_generic_count)
        .collect::<Vec<_>>();
    let actual_method_params = actual
        .generic_params
        .iter()
        .skip(comparison.impl_generic_count)
        .collect::<Vec<_>>();
    if expected_method_params.len() != actual_method_params.len() {
        diagnostics.push(mismatch("generic parameter count differs".into()));
        return;
    }
    let substitution = expected
        .generic_params()
        .iter()
        .take(comparison.trait_generic_count)
        .cloned()
        .zip(comparison.trait_arguments.iter().cloned())
        .chain(
            expected_method_params
                .into_iter()
                .zip(actual_method_params)
                .map(|(expected, actual)| (expected.clone(), TypeId::Generic(actual.clone()))),
        )
        .collect();
    for (expected_param, actual_param) in expected
        .generic_params()
        .iter()
        .skip(comparison.trait_generic_count)
        .zip(
            actual
                .generic_params
                .iter()
                .skip(comparison.impl_generic_count),
        )
    {
        let required = expected
            .bounds()
            .get(expected_param)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let provided = actual
            .bounds
            .get(actual_param)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let substituted = required
            .iter()
            .map(|constraint| match constraint {
                super::ConstraintTarget::Standard(value) => {
                    super::ConstraintTarget::Standard(*value)
                }
                super::ConstraintTarget::Trait(instance) => {
                    super::ConstraintTarget::Trait(instance.instantiate(&substitution))
                }
            })
            .collect::<Vec<_>>();
        if substituted.len() != provided.len()
            || !substituted
                .iter()
                .all(|constraint| provided.contains(constraint))
        {
            diagnostics.push(mismatch("generic bound differs".into()));
            return;
        }
    }
    for (index, actual_param) in actual.params.iter().enumerate() {
        let (name, writeability, ty) = expected.param(index);
        if writeability != actual_param.writeability {
            diagnostics.push(mismatch(format!("parameter `{name}` mutability differs")));
        }
        let expected_ty = ty
            .with_self(comparison.trait_owner, comparison.receiver)
            .instantiate(&substitution);
        if expected_ty != actual_param.ty {
            diagnostics.push(mismatch(format!(
                "parameter `{name}` expected `{}`, found `{}`",
                display_type_id(&expected_ty),
                display_type_id(&actual_param.ty)
            )));
        }
    }
    let expected_return = expected
        .return_type()
        .with_self(comparison.trait_owner, comparison.receiver)
        .instantiate(&substitution);
    if expected_return != actual.return_type {
        diagnostics.push(mismatch(format!(
            "return type expected `{}`, found `{}`",
            display_type_id(&expected_return),
            display_type_id(&actual.return_type)
        )));
    }
}

fn compare_impl_method_signature(
    function_index: &FunctionTypeIndex,
    trait_def: &crate::hir::TraitDef,
    trait_method: &crate::hir::TraitMethod,
    impl_method: &crate::hir::ImplMethod,
    receiver: (
        &TypeId,
        &kagari_common::identity::DefinitionId,
        &[TypeId],
        usize,
    ),
    span: kagari_common::Span,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    let Some(trait_function) = function_index.by_id.get(&trait_method.function) else {
        return;
    };
    let Some(impl_function) = function_index.by_id.get(&impl_method.function) else {
        return;
    };
    compare_method_contract(
        trait_function,
        impl_function,
        &MethodComparison {
            trait_name: &trait_def.name,
            method_name: &trait_method.name,
            trait_generic_count: trait_def.generic_params.len(),
            impl_generic_count: receiver.3,
            receiver: receiver.0,
            trait_owner: receiver.1,
            trait_arguments: receiver.2,
            span,
        },
        diagnostics,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConstVisitState {
    Visiting,
    Done,
}

fn validate_const_initializers(
    lowered: &LoweredModule,
    names: &ResolvedNames,
    top_level_index: &TopLevelTypeIndex,
    type_table: &TypeTable,
    cancel: &kagari_common::cancellation::CancellationToken,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    budget: &mut super::const_budget::ConstBudget,
) {
    struct ConstValidator<'a> {
        lowered: &'a LoweredModule,
        names: &'a ResolvedNames,
        top_level_index: &'a TopLevelTypeIndex,
        type_table: &'a TypeTable,
        cancel: &'a kagari_common::cancellation::CancellationToken,
        diagnostics: &'a mut SmallVec<[Diagnostic; 4]>,
        states: HashMap<ConstId, ConstVisitState>,
        budget: &'a mut super::const_budget::ConstBudget,
    }

    impl ConstValidator<'_> {
        fn validate_const(&mut self, const_id: ConstId) {
            if self.budget.exhausted || self.cancel.check().is_err() {
                return;
            }
            match self.states.get(&const_id) {
                Some(ConstVisitState::Done) => return,
                Some(ConstVisitState::Visiting) => {
                    let const_item = self.lowered.module.constant(const_id);
                    self.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::ConstCycle {
                            const_name: const_item.name.clone(),
                        })
                        .with_span(self.lowered.source_map.const_span(const_id)),
                    );
                    return;
                }
                None => {}
            }

            let initializer = self.lowered.module.constant(const_id).initializer;
            if !self.budget.enter(
                self.lowered.source_map.expr_span(initializer),
                self.diagnostics,
            ) {
                return;
            }
            self.validate_const_inner(const_id);
            self.budget.leave();
        }

        fn validate_const_inner(&mut self, const_id: ConstId) {
            self.states.insert(const_id, ConstVisitState::Visiting);
            let const_item = self.lowered.module.constant(const_id);
            if let Some(const_ty) = self.top_level_index.consts.get(&const_id)
                && !supports_const_type(const_ty)
            {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidConstInitializer {
                        const_name: const_item.name.clone(),
                        reason: format!(
                            "const type `{}` is heap-backed; const supports value types only",
                            display_type_id(const_ty)
                        ),
                    })
                    .with_span(self.lowered.source_map.const_span(const_id)),
                );
                self.states.insert(const_id, ConstVisitState::Done);
                return;
            }

            // The root was charged before checking whether its type is const-safe.
            self.validate_const_expr_inner(const_item.id, const_item.initializer);
            if self.budget.exhausted || self.cancel.check().is_err() {
                return;
            }
            let const_item = self.lowered.module.constant(const_id);
            if let (Some(declared), Some(actual)) = (
                self.top_level_index.consts.get(&const_id),
                self.type_table.expr_type(const_item.initializer),
            ) && declared.conflicts_with(&actual)
            {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidConstInitializer {
                        const_name: const_item.name.clone(),
                        reason: format!(
                            "expected `{}`, found `{}`",
                            display_type_id(declared),
                            display_type_id(&actual)
                        ),
                    })
                    .with_span(self.lowered.source_map.expr_span(const_item.initializer)),
                );
            }
            self.states.insert(const_id, ConstVisitState::Done);
        }

        fn validate_const_expr(&mut self, owner: ConstId, expr_id: ExprId) {
            if self.cancel.check().is_err()
                || !self
                    .budget
                    .enter(self.lowered.source_map.expr_span(expr_id), self.diagnostics)
            {
                return;
            }
            self.validate_const_expr_inner(owner, expr_id);
            self.budget.leave();
        }

        fn validate_const_expr_inner(&mut self, owner: ConstId, expr_id: ExprId) {
            if self.cancel.check().is_err() {
                return;
            }
            let expr = self.lowered.module.expr(expr_id);
            match &expr.kind {
                ExprKind::Literal(_) => {}
                ExprKind::Name { .. } => {
                    let Some(resolved) = self.names.expr_resolution(expr_id) else {
                        self.emit_invalid_const(
                            owner,
                            expr_id,
                            "const initializer must use literals or other consts",
                        );
                        return;
                    };

                    match resolved {
                        ResolvedName::Const(id) => self.validate_const(id),
                        _ => self.emit_invalid_const(
                            owner,
                            expr_id,
                            "const initializer must use literals or other consts",
                        ),
                    }
                }
                ExprKind::Prefix { op, expr } => {
                    self.validate_const_expr(owner, *expr);

                    let Some(expr_ty) = self.type_table.expr_type(*expr) else {
                        self.emit_invalid_const(
                            owner,
                            expr_id,
                            "const initializer has unknown operand type",
                        );
                        return;
                    };

                    let supported = match op {
                        PrefixOp::Neg => surface::supports_unary_negation(&expr_ty),
                        PrefixOp::Not => expr_ty == TypeId::Builtin(BuiltinType::Bool),
                    };
                    if !supported {
                        self.emit_invalid_const(
                            owner,
                            expr_id,
                            "unsupported unary const expression",
                        );
                    }
                }
                ExprKind::Binary { lhs, op, rhs } => {
                    self.validate_const_expr(owner, *lhs);
                    self.validate_const_expr(owner, *rhs);

                    let lhs_ty = self.type_table.expr_type(*lhs);
                    let rhs_ty = self.type_table.expr_type(*rhs);
                    if !supports_const_binary(op, lhs_ty.as_ref(), rhs_ty.as_ref()) {
                        self.emit_invalid_const(
                            owner,
                            expr_id,
                            "unsupported binary const expression",
                        );
                    }
                }
                ExprKind::Tuple(elements) | ExprKind::Array(elements) => {
                    for element in elements {
                        self.validate_const_expr(owner, *element);
                    }
                }
                ExprKind::StructInit { fields, .. } => {
                    for field in fields {
                        self.validate_const_expr(owner, field.value);
                    }
                }
                _ => self.emit_invalid_const(
                    owner,
                    expr_id,
                    "unsupported const initializer expression",
                ),
            }

            if let Some(resolved) = self.names.expr_resolution(expr_id)
                && let ResolvedName::Const(id) = resolved
                && !self.top_level_index.consts.contains_key(&id)
            {
                self.emit_invalid_const(
                    owner,
                    expr_id,
                    "const initializer references an unresolved const type",
                );
            }
        }

        fn emit_invalid_const(&mut self, owner: ConstId, expr_id: ExprId, reason: &'static str) {
            if self.budget.exhausted || self.cancel.check().is_err() {
                return;
            }
            let const_item = self.lowered.module.constant(owner);
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidConstInitializer {
                    const_name: const_item.name.clone(),
                    reason: reason.to_owned(),
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
        }
    }

    fn supports_const_binary(op: &BinaryOp, lhs: Option<&TypeId>, rhs: Option<&TypeId>) -> bool {
        match (op, lhs, rhs) {
            (
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem,
                Some(lhs),
                Some(rhs),
            ) => surface::supports_arithmetic(lhs, rhs),
            (BinaryOp::Eq | BinaryOp::NotEq, Some(lhs), Some(rhs)) => lhs == rhs,
            (BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge, Some(lhs), Some(rhs)) => {
                surface::supports_ordering(lhs, rhs)
            }
            (
                BinaryOp::AndAnd | BinaryOp::OrOr,
                Some(TypeId::Builtin(BuiltinType::Bool)),
                Some(TypeId::Builtin(BuiltinType::Bool)),
            ) => true,
            _ => false,
        }
    }

    fn supports_const_type(ty: &TypeId) -> bool {
        surface::supports_const_type(ty)
    }

    let mut validator = ConstValidator {
        lowered,
        names,
        top_level_index,
        type_table,
        cancel,
        diagnostics,
        states: HashMap::new(),
        budget,
    };
    for const_item in &lowered.module.consts {
        if validator.budget.exhausted || cancel.check().is_err() {
            break;
        }
        validator.validate_const(const_item.id);
    }
}

#[cfg(test)]
mod constraint_traversal_tests {
    use super::*;
    use kagari_common::cancellation::CancellationToken;

    #[test]
    fn deep_container_validation_uses_explicit_stack_and_honors_cancellation() {
        let mut ty = TypeId::Map {
            key: Box::new(TypeId::Builtin(BuiltinType::F32)),
            value: Box::new(TypeId::Error),
        };
        for _ in 0..10_000 {
            ty = TypeId::Array(Box::new(ty));
        }
        let mut diagnostics = SmallVec::new();
        let cancelled = CancellationToken::default();
        cancelled.cancel();
        validate_standard_type_constraints(
            &ty,
            &HashMap::new(),
            Default::default(),
            &mut diagnostics,
            &cancelled,
        );
        let cancelled_count = diagnostics.len();
        validate_standard_type_constraints(
            &ty,
            &HashMap::new(),
            Default::default(),
            &mut diagnostics,
            &Default::default(),
        );
        // Drop the synthetic deep input iteratively as well: this test exercises
        // validation, not the recursive representation's destructor.
        while let TypeId::Array(inner) = ty {
            ty = *inner;
        }
        assert_eq!(cancelled_count, 0);
        assert_eq!(diagnostics.len(), 1);
        assert!(matches!(&diagnostics[0].kind,
            DiagnosticKind::StandardConstraintNotSatisfied { type_name, .. } if type_name == "f32"
        ));
    }
}

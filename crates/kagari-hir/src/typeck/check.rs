//! Signature/body phase orchestration. Signature checking resolves declaration types
//! and constraints; body checking starts from those facts, evaluates constant
//! prerequisites, then checks or restores selected functions into a new type table.

use crate::{
    AnalysisResult,
    aggregates::{AggregateCatalog, traits::MethodSignature},
    declarations::Declarations,
    hir::{
        ids::BodySelection,
        item::{
            Module,
            function::{Function, FunctionKind},
        },
        writeability::Writeability,
    },
    lower::LoweredModule,
    native::NativeBinding,
    resolver::resolved::{ResolvedName, ResolvedNames},
    typeck::{
        BodyTypeEnv, FunctionImplementation, FunctionTypeIndex, TopLevelTypeIndex, TypeIndexes,
        TypedFunction, TypedFunctionBuffer, TypedModule, TypedParameter, TypedParameterBuffer,
        associated, associated_consts,
        body::BodyChecker,
        check::{
            constants::validate_const_initializers, native_defaults::NativeDefaultCheck,
            trait_surface::validate_trait_surface,
        },
        completion,
        const_budget::ConstBudget,
        const_eval, constraints,
        reuse::BodyReuse,
        table::{ConstraintTarget, TypeTable},
        ty::{TypeContext, display_type, display_type_id, resolve_type, resolve_type_in},
    },
    types::{GenericParameterType, NominalType, TypeId, TypeSubstitution},
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionPath, span::Span};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind, Severity, TypePosition};
use kagari_types::{
    scalar::BuiltinType, surface as standard_surface, surface::StandardTypeConstraint,
    visibility::Visibility,
};
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};

mod constants;
mod native_defaults;
mod trait_surface;

#[cfg(test)]
use kagari_types::collection::CollectionAccess;

/// Resolves declaration types, generic bounds and associated members into reusable signature facts.
pub(crate) fn check_signatures(
    lowered: &LoweredModule,
    declarations: &Declarations,
    cancel: &CancellationToken,
) -> AnalysisResult<super::ModuleSignatures> {
    let mut diagnostics = SmallVec::<[Diagnostic; 4]>::new();
    let mut functions: TypedFunctionBuffer = SmallVec::new();
    let mut function_index = FunctionTypeIndex::default();
    let mut type_table = TypeTable::default();
    constraints::resolve_constraints(
        lowered,
        declarations,
        &mut type_table,
        &mut diagnostics,
        cancel,
    );
    associated::prepare(
        lowered,
        declarations,
        &mut type_table,
        &mut diagnostics,
        cancel,
    );
    associated_consts::prepare(
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
        .chain(
            lowered
                .module
                .opaque_types
                .iter()
                .map(|item| (ResolvedName::OpaqueType(item.id), &item.generic_params)),
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
            constraints::parameter_bounds(params, declarations, &type_table),
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
                    implementation: None,
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
                    None,
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
                        implementation: None,
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
                        None,
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
                            None,
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
                    implementation: None,
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
        let implementation = match lowered.native_functions.get(&function.id) {
            Some(binding) => FunctionImplementation::Native(binding.clone()),
            None if function.body.is_some() => FunctionImplementation::Script,
            None => FunctionImplementation::Required,
        };
        let inherent_member = lowered.module.impls.iter().any(|implementation| {
            implementation.trait_ref.is_none()
                && implementation
                    .methods
                    .iter()
                    .any(|method| method.function == function.id)
        });
        if function.visibility != Visibility::Private
            && !inherent_member
            && !function.generic_params.is_empty()
            && !matches!(
                implementation,
                FunctionImplementation::Native(NativeBinding::Entry(_))
            )
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
        let bounds =
            constraints::function_bounds(&lowered.module, function, declarations, &type_table);
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
                None,
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
                    None,
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
            implementation,
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

/// Checks constant prerequisites and selected function bodies using completed signature inputs.
///
/// Copies signature facts into the output table, restores compatible body facts where
/// possible, and records fresh diagnostics/types for remaining bodies. Caller-owned
/// cancellation and policy limits apply throughout; publication occurs above this layer.
pub(crate) fn check_bodies_controlled(
    lowered: &LoweredModule,
    names: &ResolvedNames,
    declarations: &Declarations,
    inputs: super::BodyInputs<'_>,
    reuse: Option<&BodyReuse<'_>>,
    cancel: &CancellationToken,
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
    let mut diagnostics = if matches!(selection, BodySelection::All) {
        signatures.diagnostics.clone()
    } else {
        Default::default()
    };
    let mut functions = signatures.facts.functions.clone();
    for function in &mut functions {
        for param in &mut function.params {
            param.ty = aggregates.normalize_type(&param.ty);
        }
        function.return_type = aggregates.normalize_type(&function.return_type);
    }
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
                        None,
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
                        None,
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
                let _ =
                    checker.infer_expr_type_expected(const_item.initializer, &mut env, Some(&ty));
            }
            top_level_index.consts.insert(const_item.id, ty.clone());
        }

        let mut const_budget = ConstBudget::new(const_limits);
        validate_const_initializers(
            lowered,
            names,
            &top_level_index,
            &type_table,
            cancel,
            &mut diagnostics,
            &mut const_budget,
        );

        let const_values = const_eval::evaluate_constants(
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
            let Some(body) = function.body else {
                continue;
            };
            if matches!(function.kind, FunctionKind::TraitMethod)
                && !lowered
                    .module
                    .traits
                    .iter()
                    .flat_map(|item| &item.methods)
                    .any(|method| method.function == function.id && method.has_default)
            {
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
                let context = function_type_context(&lowered.module, function, declarations);
                env.self_type = if let Some(owner) = context.self_type {
                    declarations
                        .definition(ResolvedName::Trait(owner))
                        .cloned()
                        .map(TypeId::SelfType)
                } else {
                    context
                        .implementation
                        .and_then(|owner| lowered.module.impls.iter().find(|item| item.id == owner))
                        .and_then(|item| item.for_type)
                        .and_then(|ty| type_table.type_ref(ty))
                        .map(|ty| ty.ty.clone())
                };
                env.generic_bounds = aggregates
                    .expanded_bounds(&typed_function.bounds, cancel)
                    .unwrap_or_else(|_| typed_function.bounds.clone());
                if let Some(contract) = lowered
                    .module
                    .traits
                    .iter()
                    .find(|item| {
                        item.methods
                            .iter()
                            .any(|method| method.function == function.id)
                    })
                    .and_then(|item| declarations.definition(ResolvedName::Trait(item.id)))
                    .and_then(|id| aggregates.trait_(id))
                {
                    let receiver = TypeId::SelfType(contract.id.clone());
                    let applied = NominalType {
                        declaration: contract.id.clone(),
                        arguments: contract
                            .generic_params
                            .iter()
                            .cloned()
                            .map(TypeId::Generic)
                            .collect(),
                        associated_types: Default::default(),
                    };
                    if let Ok(parents) = aggregates.trait_closure(&applied, &receiver, cancel) {
                        env.generic_bounds
                            .entry(receiver)
                            .or_default()
                            .extend(parents.into_iter().map(ConstraintTarget::Trait));
                    }
                }
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
                let body_ty = checker.solve_body(body, &mut env, Some(&typed_function.return_type));
                let Ok(completes) = completion::block_can_complete(
                    &lowered.module,
                    names,
                    &type_table,
                    body,
                    cancel,
                ) else {
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

        let defaults = NativeDefaultCheck {
            lowered,
            names,
            declarations,
            imports: imported_functions,
            aggregates,
        };
        for function in &lowered.module.functions {
            // Recovery facts never authorize direct native lowering. In particular,
            // a matching call signature does not prove its generic obligations.
            if diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error)
            {
                break;
            }
            if !selection.includes(function.id) || cancel.check().is_err() {
                continue;
            }
            let Some(NativeBinding::Default(application)) =
                lowered.native_functions.get(&function.id)
            else {
                continue;
            };
            let Some(signature) = function_index.by_id.get(&function.id) else {
                continue;
            };
            if let Some(site) =
                defaults.forwarding_call(function, signature, application, &type_table)
            {
                type_table.native_default_calls.insert(function.id, site);
            } else {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidNativeSignature {
                        function: function.name.clone(),
                        binding: "checked native default forwarding body".into(),
                    })
                    .with_span(lowered.source_map.function_span(function.id)),
                );
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

pub(super) fn function_type_context<'a>(
    module: &'a Module,
    function: &'a Function,
    declarations: &'a Declarations,
) -> TypeContext<'a> {
    TypeContext {
        declarations,
        generics: &function.generic_params,
        implementation: module
            .impls
            .iter()
            .find(|item| {
                item.methods
                    .iter()
                    .any(|method| method.function == function.id)
            })
            .map(|item| item.id),
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

        (TypeId::Tuple(left), TypeId::Tuple(right)) => left.len() == right.len(),
        (TypeId::Function { params: left, .. }, TypeId::Function { params: right, .. }) => {
            left.len() == right.len()
        }
        (TypeId::Range(_, left), TypeId::Range(_, right)) => left == right,
        (TypeId::Iter(_), TypeId::Iter(_))
        | (TypeId::Array(_, _), TypeId::Array(_, _))
        | (TypeId::Set(_, _), TypeId::Set(_, _))
        | (TypeId::Map { .. }, TypeId::Map { .. }) => true,
        _ => false,
    }
}

pub(super) fn validate_standard_type_constraints(
    ty: &TypeId,
    generic_bounds: &HashMap<TypeId, Vec<ConstraintTarget>>,
    span: Span,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &CancellationToken,
    catalog: Option<&AggregateCatalog>,
) {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if cancel.check().is_err() {
            return;
        }
        match ty {
            TypeId::Map { key, value, .. } => {
                validate_standard_constraint_type(
                    key,
                    StandardTypeConstraint::HashKey,
                    generic_bounds,
                    span,
                    diagnostics,
                    catalog,
                );
                pending.push(value);
                pending.push(key);
            }
            TypeId::Set(element, _) => {
                validate_standard_constraint_type(
                    element,
                    StandardTypeConstraint::HashKey,
                    generic_bounds,
                    span,
                    diagnostics,
                    catalog,
                );
                pending.push(element);
            }
            TypeId::Tuple(elements) => pending.extend(elements.iter().rev()),
            TypeId::Function { params, result } => {
                pending.push(result);
                pending.extend(params.iter().rev());
            }
            TypeId::Array(element, _) | TypeId::Iter(element) | TypeId::Range(element, _) => {
                pending.push(element)
            }
            TypeId::Struct(NominalType {
                arguments: args, ..
            })
            | TypeId::Enum(NominalType {
                arguments: args, ..
            })
            | TypeId::Trait(NominalType {
                arguments: args, ..
            }) => pending.extend(args.iter().rev()),
            _ => {}
        }
    }
}

pub(super) fn validate_standard_constraint_type(
    ty: &TypeId,
    constraint: StandardTypeConstraint,
    generic_bounds: &HashMap<TypeId, Vec<ConstraintTarget>>,
    span: Span,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    catalog: Option<&AggregateCatalog>,
) {
    if matches!(ty, TypeId::Unknown | TypeId::Error) {
        return;
    }
    if constraint == StandardTypeConstraint::Comparable && ty.is_unresolved() {
        return;
    }
    // Header checking has not assembled installed trait facts yet. Nominal
    // interface value constraints are completed with the aggregate catalog.
    if catalog.is_none() && ty.contains_interface() {
        return;
    }
    if constraints::type_satisfies_standard_constraint(ty, constraint, generic_bounds, catalog) {
        return;
    }

    diagnostics.push(
        Diagnostic::error(DiagnosticKind::StandardConstraintNotSatisfied {
            type_name: display_type_id(ty),
            constraint: standard_surface::standard_constraint_name(constraint).to_owned(),
            reason: constraints::standard_constraint_reason(constraint).to_owned(),
        })
        .with_span(span),
    );
}

pub(super) fn interface_method_compatible<'a>(
    has_receiver: bool,
    return_type: &TypeId,
    other_parameters: impl IntoIterator<Item = &'a TypeId>,
) -> bool {
    has_receiver
        && !return_type.contains_self_type()
        && other_parameters
            .into_iter()
            .all(|parameter| !parameter.contains_self_type())
}

/// Shared read-only signature contract for comparing source and catalog methods.
pub(super) trait MethodSignatureView {
    fn generic_params(&self) -> &[GenericParameterType];

    fn bounds(&self) -> &super::GenericBounds;

    fn params_len(&self) -> usize;

    fn param(&self, index: usize) -> (&str, Writeability, &TypeId);

    fn return_type(&self) -> &TypeId;
}

impl MethodSignatureView for TypedFunction {
    fn generic_params(&self) -> &[GenericParameterType] {
        &self.generic_params
    }

    fn bounds(&self) -> &super::GenericBounds {
        &self.bounds
    }

    fn params_len(&self) -> usize {
        self.params.len()
    }

    fn param(&self, index: usize) -> (&str, Writeability, &TypeId) {
        let param = &self.params[index];
        (&param.name, param.writeability, &param.ty)
    }

    fn return_type(&self) -> &TypeId {
        &self.return_type
    }
}

impl MethodSignatureView for MethodSignature {
    fn generic_params(&self) -> &[GenericParameterType] {
        &self.generic_params
    }

    fn bounds(&self) -> &super::GenericBounds {
        &self.bounds
    }

    fn params_len(&self) -> usize {
        self.params.len()
    }

    fn param(&self, index: usize) -> (&str, Writeability, &TypeId) {
        let param = &self.params[index];
        (&param.name, param.writeability, &param.ty)
    }

    fn return_type(&self) -> &TypeId {
        &self.return_type
    }
}

/// Receiver, owner substitution and diagnostic site used to compare one implementation method.
pub(super) struct MethodComparison<'a> {
    pub trait_name: &'a str,
    pub method_name: &'a str,
    pub trait_generic_count: usize,
    pub impl_generic_count: usize,
    pub receiver: &'a TypeId,
    pub trait_owner: &'a DefinitionPath,
    pub trait_arguments: &'a [TypeId],
    pub catalog: &'a AggregateCatalog,
    pub span: Span,
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
    let mut substitution: TypeSubstitution = expected
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
    substitution.insert_receiver(comparison.trait_owner.clone(), comparison.receiver.clone());
    let normalize = |ty: TypeId| comparison.catalog.normalize_type(&ty);
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
            .get(&TypeId::Generic(expected_param.clone()))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let provided = actual
            .bounds
            .get(&TypeId::Generic(actual_param.clone()))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let substituted = required
            .iter()
            .map(|constraint| match constraint {
                ConstraintTarget::Standard(value) => ConstraintTarget::Standard(*value),
                ConstraintTarget::Trait(instance) => {
                    ConstraintTarget::Trait(instance.instantiate(&substitution))
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
        let expected_ty = normalize(
            ty.with_self(comparison.trait_owner, comparison.receiver)
                .instantiate(&substitution),
        );
        if expected_ty != normalize(actual_param.ty.clone()) {
            diagnostics.push(mismatch(format!(
                "parameter `{name}` expected `{}`, found `{}`",
                display_type_id(&expected_ty),
                display_type_id(&actual_param.ty)
            )));
        }
    }
    let projected_required = expected
        .bounds()
        .iter()
        .filter(|(ty, _)| matches!(ty, TypeId::Projection { .. }))
        .map(|(ty, constraints)| {
            (
                normalize(
                    ty.with_self(comparison.trait_owner, comparison.receiver)
                        .instantiate(&substitution),
                ),
                constraints
                    .iter()
                    .map(|constraint| match constraint {
                        ConstraintTarget::Standard(value) => ConstraintTarget::Standard(*value),
                        ConstraintTarget::Trait(instance) => {
                            ConstraintTarget::Trait(instance.instantiate(&substitution))
                        }
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let projected_actual = actual
        .bounds
        .iter()
        .filter(|(ty, _)| {
            matches!(ty, TypeId::Projection { .. }) || projected_required.contains_key(*ty)
        })
        .map(|(ty, constraints)| (normalize(ty.clone()), constraints.clone()))
        .collect::<HashMap<_, _>>();
    if projected_required != projected_actual {
        diagnostics.push(mismatch("associated type bound differs".into()));
    }
    let expected_return = normalize(
        expected
            .return_type()
            .with_self(comparison.trait_owner, comparison.receiver)
            .instantiate(&substitution),
    );
    if expected_return != normalize(actual.return_type.clone()) {
        diagnostics.push(mismatch(format!(
            "return type expected `{}`, found `{}`",
            display_type_id(&expected_return),
            display_type_id(&actual.return_type)
        )));
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
            access: CollectionAccess::Mutable,
        };
        for _ in 0..10_000 {
            ty = TypeId::Array(Box::new(ty), CollectionAccess::Mutable);
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
            None,
        );
        let cancelled_count = diagnostics.len();
        validate_standard_type_constraints(
            &ty,
            &HashMap::new(),
            Default::default(),
            &mut diagnostics,
            &Default::default(),
            None,
        );
        // Drop the synthetic deep input iteratively as well: this test exercises
        // validation, not the recursive representation's destructor.
        while let TypeId::Array(inner, _) = ty {
            ty = *inner;
        }
        assert_eq!(cancelled_count, 0);
        assert_eq!(diagnostics.len(), 1);
        assert!(matches!(&diagnostics[0].kind,
            DiagnosticKind::StandardConstraintNotSatisfied { type_name, .. } if type_name == "f32"
        ));
    }
}

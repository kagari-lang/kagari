use kagari_common::{Diagnostic, DiagnosticKind, TypePosition};
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};

use crate::{
    BoxedDiagnosticBuffer,
    builtin::surface::{self, StandardTypeConstraint},
    hir::FunctionKind,
    hir::{BinaryOp, ConstId, ConstItem, ExprId, ExprKind, PrefixOp},
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

pub fn check_module(
    lowered: &LoweredModule,
    names: &ResolvedNames,
) -> Result<TypedModule, BoxedDiagnosticBuffer> {
    let mut diagnostics = SmallVec::<[Diagnostic; 4]>::new();
    let mut functions: TypedFunctionBuffer = SmallVec::new();
    let mut function_index = FunctionTypeIndex::default();
    let mut top_level_index = TopLevelTypeIndex::default();

    for function in &lowered.module.functions {
        let mut params: TypedParameterBuffer = SmallVec::new();
        let generic_names = function_generic_names(function);
        let context = TypeContext {
            generics: &generic_names,
        };
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

            match resolve_type_in(&lowered.module, param.ty, context) {
                Some(ty) => {
                    validate_standard_type_constraints(
                        &ty,
                        &function_bounds(function),
                        lowered.source_map.type_span(param.ty),
                        &mut diagnostics,
                    );
                    params.push(TypedParameter {
                        id: param.id,
                        writeability: param.writeability,
                        name: param_name,
                        ty,
                    });
                }
                None => diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownType {
                        type_name: param_ty_name,
                        function_name: function_name.clone(),
                        position: TypePosition::Parameter,
                    })
                    .with_span(lowered.source_map.type_span(param.ty)),
                ),
            }
        }

        let return_type = match &function.return_type {
            Some(ty_ref) => match resolve_type_in(&lowered.module, *ty_ref, context) {
                Some(ty) => {
                    validate_standard_type_constraints(
                        &ty,
                        &function_bounds(function),
                        lowered.source_map.type_span(*ty_ref),
                        &mut diagnostics,
                    );
                    ty
                }
                None => {
                    let ty_name = display_type(&lowered.module, *ty_ref);
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnknownType {
                            type_name: ty_name,
                            function_name: function_name.clone(),
                            position: TypePosition::Return,
                        })
                        .with_span(lowered.source_map.type_span(*ty_ref)),
                    );
                    TypeId::Builtin(BuiltinType::Unit)
                }
            },
            None => TypeId::Builtin(BuiltinType::Unit),
        };

        let typed_function = TypedFunction {
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

    if !diagnostics.is_empty() {
        Err(Box::new(diagnostics))
    } else {
        let mut type_table = TypeTable::default();
        for const_item in &lowered.module.consts {
            let ty = match const_item.ty {
                Some(ty_ref) => match resolve_type(&lowered.module, ty_ref) {
                    Some(ty) => {
                        validate_standard_type_constraints(
                            &ty,
                            &HashMap::new(),
                            lowered.source_map.type_span(ty_ref),
                            &mut diagnostics,
                        );
                        ty
                    }
                    None => {
                        diagnostics.push(
                            Diagnostic::error(DiagnosticKind::UnknownConstType {
                                const_name: const_item.name.clone(),
                                type_name: display_type(&lowered.module, ty_ref),
                            })
                            .with_span(lowered.source_map.const_span(const_item.id)),
                        );
                        TypeId::Builtin(BuiltinType::Unit)
                    }
                },
                None => {
                    let mut env = BodyTypeEnv::default();
                    let mut checker = BodyChecker::new(
                        lowered,
                        names,
                        TypeIndexes {
                            function_index: &function_index,
                            top_level_index: &top_level_index,
                        },
                        &mut diagnostics,
                        &mut type_table,
                        "<const>",
                        TypeId::Builtin(BuiltinType::Unit),
                    );
                    checker.infer_expr_type(const_item.initializer, &mut env)
                }
            };
            if const_item.ty.is_some() {
                let mut env = BodyTypeEnv::default();
                let mut checker = BodyChecker::new(
                    lowered,
                    names,
                    TypeIndexes {
                        function_index: &function_index,
                        top_level_index: &top_level_index,
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

        validate_const_initializers(
            lowered,
            names,
            &top_level_index,
            &type_table,
            &mut diagnostics,
        );
        validate_trait_surface(lowered, &function_index, &mut diagnostics);

        for function in &lowered.module.functions {
            if matches!(function.kind, FunctionKind::TraitMethod) {
                continue;
            }
            let mut env = BodyTypeEnv::default();
            if let Some(typed_function) = function_index.by_id.get(&function.id) {
                env.generics = function_generic_names(function);
                env.generic_bounds = function_bounds(function);
                for param in &typed_function.params {
                    env.params.insert(param.id, param.ty.clone());
                }
                let mut checker = BodyChecker::new(
                    lowered,
                    names,
                    TypeIndexes {
                        function_index: &function_index,
                        top_level_index: &top_level_index,
                    },
                    &mut diagnostics,
                    &mut type_table,
                    &typed_function.name,
                    typed_function.return_type.clone(),
                );
                let body_ty = checker.infer_block_types(function.body, &mut env);
                if matches!(function.kind, FunctionKind::ModuleInit) {
                    if let Some(indexed) = function_index.by_id.get_mut(&function.id) {
                        indexed.return_type = body_ty.clone();
                    }
                    for typed in &mut functions {
                        if typed.id == function.id {
                            typed.return_type = body_ty.clone();
                            break;
                        }
                    }
                    continue;
                }
                if body_ty != typed_function.return_type {
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

        if diagnostics.is_empty() {
            Ok(TypedModule {
                functions,
                consts: top_level_index.consts,
                type_table,
            })
        } else {
            Err(Box::new(diagnostics))
        }
    }
}

fn function_generic_names(function: &crate::hir::Function) -> Vec<String> {
    let mut names = function
        .generic_params
        .iter()
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
    if matches!(function.kind, FunctionKind::TraitMethod) {
        names.push("Self".to_string());
    }
    names
}

fn function_bounds(function: &crate::hir::Function) -> HashMap<String, Vec<String>> {
    let mut bounds = HashMap::<String, Vec<String>>::new();
    for param in &function.generic_params {
        let entry = bounds.entry(param.name.clone()).or_default();
        entry.extend(param.bounds.iter().map(|bound| bound.name.clone()));
    }
    for bound in &function.bounds {
        let entry = bounds.entry(bound.target.clone()).or_default();
        entry.extend(bound.traits.iter().map(|trait_ref| trait_ref.name.clone()));
    }
    bounds
}

fn validate_trait_surface(
    lowered: &LoweredModule,
    function_index: &FunctionTypeIndex,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    let trait_names = lowered
        .module
        .traits
        .iter()
        .map(|trait_def| trait_def.name.as_str())
        .collect::<HashSet<_>>();

    for function in &lowered.module.functions {
        validate_trait_refs(
            lowered,
            &trait_names,
            &function.generic_params,
            &function.bounds,
            lowered.source_map.function_span(function.id),
            diagnostics,
        );
    }
    for trait_def in &lowered.module.traits {
        validate_trait_refs(
            lowered,
            &trait_names,
            &trait_def.generic_params,
            &[],
            lowered.source_map.trait_span(trait_def.id),
            diagnostics,
        );
    }

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
                function_index,
                ty,
                lowered.source_map.function_span(function.id),
                diagnostics,
            );
        }
    }

    let mut seen_impls = HashSet::<(String, String)>::new();
    for impl_block in &lowered.module.impls {
        validate_trait_refs(
            lowered,
            &trait_names,
            &impl_block.generic_params,
            &impl_block.bounds,
            lowered.source_map.impl_span(impl_block.id),
            diagnostics,
        );

        let Some(trait_name) = impl_block.trait_ref.as_deref() else {
            continue;
        };
        let Some(trait_def) = lowered
            .module
            .traits
            .iter()
            .find(|trait_def| trait_def.name == trait_name)
        else {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::UnknownTrait {
                    trait_name: trait_name.to_string(),
                })
                .with_span(lowered.source_map.impl_span(impl_block.id)),
            );
            continue;
        };

        let generic_names = impl_block
            .generic_params
            .iter()
            .map(|param| param.name.clone())
            .collect::<Vec<_>>();
        let Some(for_ty) = impl_block.for_type.and_then(|ty| {
            resolve_type_in(
                &lowered.module,
                ty,
                TypeContext {
                    generics: &generic_names,
                },
            )
        }) else {
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

        if !seen_impls.insert((trait_name.to_string(), type_name.clone())) {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                    trait_name: trait_name.to_string(),
                    type_name: type_name.clone(),
                    reason: "duplicate impl".to_string(),
                })
                .with_span(lowered.source_map.impl_span(impl_block.id)),
            );
        }

        validate_impl_methods(
            lowered,
            function_index,
            trait_def,
            impl_block,
            &for_ty,
            diagnostics,
        );
    }
}

fn validate_trait_refs(
    _lowered: &LoweredModule,
    trait_names: &HashSet<&str>,
    generic_params: &[crate::hir::GenericParam],
    bounds: &[crate::hir::TraitBound],
    span: kagari_common::Span,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    for param in generic_params {
        for trait_ref in &param.bounds {
            if surface::standard_constraint(&trait_ref.name).is_some() {
                continue;
            }
            if !trait_names.contains(trait_ref.name.as_str()) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownTrait {
                        trait_name: trait_ref.name.clone(),
                    })
                    .with_span(span),
                );
            }
        }
    }
    for bound in bounds {
        for trait_ref in &bound.traits {
            if surface::standard_constraint(&trait_ref.name).is_some() {
                continue;
            }
            if !trait_names.contains(trait_ref.name.as_str()) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownTrait {
                        trait_name: trait_ref.name.clone(),
                    })
                    .with_span(span),
                );
            }
        }
    }
}

fn validate_standard_type_constraints(
    ty: &TypeId,
    generic_bounds: &HashMap<String, Vec<String>>,
    span: kagari_common::Span,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    match ty {
        TypeId::Map { key, value } => {
            validate_standard_constraint_type(
                key,
                StandardTypeConstraint::HashKey,
                generic_bounds,
                span,
                diagnostics,
            );
            validate_standard_type_constraints(value, generic_bounds, span, diagnostics);
        }
        TypeId::Set(element) => {
            validate_standard_constraint_type(
                element,
                StandardTypeConstraint::HashKey,
                generic_bounds,
                span,
                diagnostics,
            );
        }
        TypeId::Tuple(elements) => {
            for element in elements {
                validate_standard_type_constraints(element, generic_bounds, span, diagnostics);
            }
        }
        TypeId::Array(element) => {
            validate_standard_type_constraints(element, generic_bounds, span, diagnostics);
        }
        TypeId::StandardEnum { args, .. } => {
            for arg in args {
                validate_standard_type_constraints(arg, generic_bounds, span, diagnostics);
            }
        }
        _ => {}
    }
}

fn validate_standard_constraint_type(
    ty: &TypeId,
    constraint: StandardTypeConstraint,
    generic_bounds: &HashMap<String, Vec<String>>,
    span: kagari_common::Span,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    let ok = match ty {
        TypeId::Generic(name) => generic_bounds.get(name).is_some_and(|bounds| {
            bounds
                .iter()
                .any(|bound| surface::standard_constraint(bound) == Some(constraint))
        }),
        _ => match constraint {
            StandardTypeConstraint::HashKey => surface::supports_hash_key(ty),
            StandardTypeConstraint::Iterable => surface::iterable_protocol(ty).is_some(),
            StandardTypeConstraint::OrderedNumber => surface::supports_ordering(ty, ty),
            StandardTypeConstraint::SignedNumber => surface::supports_unary_negation(ty),
            StandardTypeConstraint::Comparable => true,
        },
    };

    if ok {
        return;
    }

    diagnostics.push(
        Diagnostic::error(DiagnosticKind::StandardConstraintNotSatisfied {
            type_name: display_type_id(ty),
            constraint: surface::standard_constraint_name(constraint).to_owned(),
            reason: "standard collection keys must have specified hash semantics".to_owned(),
        })
        .with_span(span),
    );
}

fn trait_method_interface_compatible(
    lowered: &LoweredModule,
    function_index: &FunctionTypeIndex,
    function_id: crate::hir::FunctionId,
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
    hir_function.generic_params.is_empty()
        && function.params.iter().any(|param| param.name == "self")
        && !matches!(function.return_type, TypeId::Generic(ref name) if name == "Self")
}

fn validate_interface_type(
    lowered: &LoweredModule,
    function_index: &FunctionTypeIndex,
    ty: &TypeId,
    span: kagari_common::Span,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    match ty {
        TypeId::Trait(trait_name) => {
            if let Some(trait_def) = lowered
                .module
                .traits
                .iter()
                .find(|trait_def| trait_def.name == *trait_name)
            {
                for method in &trait_def.methods {
                    if !trait_method_interface_compatible(lowered, function_index, method.function)
                    {
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
                validate_interface_type(lowered, function_index, element, span, diagnostics);
            }
        }
        TypeId::Array(element) => {
            validate_interface_type(lowered, function_index, element, span, diagnostics);
        }
        TypeId::Map { key, value } => {
            validate_interface_type(lowered, function_index, key, span, diagnostics);
            validate_interface_type(lowered, function_index, value, span, diagnostics);
        }
        TypeId::Set(element) => {
            validate_interface_type(lowered, function_index, element, span, diagnostics);
        }
        TypeId::StandardEnum { args, .. } => {
            for arg in args {
                validate_interface_type(lowered, function_index, arg, span, diagnostics);
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
    for_ty: &TypeId,
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
            for_ty,
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

fn compare_impl_method_signature(
    function_index: &FunctionTypeIndex,
    trait_def: &crate::hir::TraitDef,
    trait_method: &crate::hir::TraitMethod,
    impl_method: &crate::hir::ImplMethod,
    for_ty: &TypeId,
    span: kagari_common::Span,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    let Some(trait_function) = function_index.by_id.get(&trait_method.function) else {
        return;
    };
    let Some(impl_function) = function_index.by_id.get(&impl_method.function) else {
        return;
    };
    if trait_function.params.len() != impl_function.params.len() {
        diagnostics.push(
            Diagnostic::error(DiagnosticKind::TraitMethodMismatch {
                trait_name: trait_def.name.clone(),
                method_name: trait_method.name.clone(),
                reason: "parameter count differs".to_string(),
            })
            .with_span(span),
        );
        return;
    }
    for (trait_param, impl_param) in trait_function.params.iter().zip(&impl_function.params) {
        let expected = substitute_self_type(&trait_param.ty, for_ty);
        if expected != impl_param.ty {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::TraitMethodMismatch {
                    trait_name: trait_def.name.clone(),
                    method_name: trait_method.name.clone(),
                    reason: format!(
                        "parameter `{}` expected `{}`, found `{}`",
                        trait_param.name,
                        display_type_id(&expected),
                        display_type_id(&impl_param.ty)
                    ),
                })
                .with_span(span),
            );
        }
    }
    let expected_return = substitute_self_type(&trait_function.return_type, for_ty);
    if expected_return != impl_function.return_type {
        diagnostics.push(
            Diagnostic::error(DiagnosticKind::TraitMethodMismatch {
                trait_name: trait_def.name.clone(),
                method_name: trait_method.name.clone(),
                reason: format!(
                    "return type expected `{}`, found `{}`",
                    display_type_id(&expected_return),
                    display_type_id(&impl_function.return_type)
                ),
            })
            .with_span(span),
        );
    }
}

fn substitute_self_type(ty: &TypeId, self_ty: &TypeId) -> TypeId {
    match ty {
        TypeId::Generic(name) if name == "Self" => self_ty.clone(),
        TypeId::Tuple(elements) => TypeId::Tuple(
            elements
                .iter()
                .map(|element| substitute_self_type(element, self_ty))
                .collect::<Vec<_>>(),
        ),
        TypeId::Array(element) => TypeId::Array(Box::new(substitute_self_type(element, self_ty))),
        TypeId::Map { key, value } => TypeId::Map {
            key: Box::new(substitute_self_type(key, self_ty)),
            value: Box::new(substitute_self_type(value, self_ty)),
        },
        TypeId::Set(element) => TypeId::Set(Box::new(substitute_self_type(element, self_ty))),
        TypeId::StandardEnum { name, args } => TypeId::StandardEnum {
            name: name.clone(),
            args: args
                .iter()
                .map(|arg| substitute_self_type(arg, self_ty))
                .collect::<Vec<_>>(),
        },
        _ => ty.clone(),
    }
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
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    struct ConstValidator<'a> {
        lowered: &'a LoweredModule,
        names: &'a ResolvedNames,
        top_level_index: &'a TopLevelTypeIndex,
        type_table: &'a TypeTable,
        diagnostics: &'a mut SmallVec<[Diagnostic; 4]>,
        states: HashMap<ConstId, ConstVisitState>,
    }

    impl ConstValidator<'_> {
        fn validate_const(&mut self, const_id: ConstId) {
            match self.states.get(&const_id) {
                Some(ConstVisitState::Done) => return,
                Some(ConstVisitState::Visiting) => {
                    let const_item = self.const_item(const_id);
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

            self.states.insert(const_id, ConstVisitState::Visiting);
            let const_item = self.const_item(const_id);
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

            self.validate_const_expr(const_item.id, const_item.initializer);
            self.states.insert(const_id, ConstVisitState::Done);
        }

        fn validate_const_expr(&mut self, owner: ConstId, expr_id: ExprId) {
            let expr = self.lowered.module.expr(expr_id);
            match &expr.kind {
                ExprKind::Literal(_) => {}
                ExprKind::Name(_) => {
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
            let const_item = self.const_item(owner);
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidConstInitializer {
                    const_name: const_item.name.clone(),
                    reason: reason.to_owned(),
                })
                .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
        }

        fn const_item(&self, const_id: ConstId) -> &ConstItem {
            self.lowered
                .module
                .consts
                .iter()
                .find(|item| item.id == const_id)
                .expect("const id should exist")
        }
    }

    fn supports_const_binary(op: &BinaryOp, lhs: Option<&TypeId>, rhs: Option<&TypeId>) -> bool {
        match (op, lhs, rhs) {
            (
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div,
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
        diagnostics,
        states: HashMap::new(),
    };
    for const_item in &lowered.module.consts {
        validator.validate_const(const_item.id);
    }
}

use crate::source::lower::{
    MirLoweringError,
    instances::{Instance, InstancePlanner},
    state::FunctionLowerer,
};
use hir::ExprKind;
use kagari_abi::{
    representation::ValueType,
    scalar::BuiltinType,
    standard::{surface::StandardEnum, traits::StandardTrait},
};
use kagari_hir::{
    AnalyzedModule,
    builtin::traits::{StandardTraitSemantics, callable_signature},
    hir,
    resolver::ResolvedName,
    typeck::TypedFunction,
    types::{TypeId, TypeSubstitution},
};
use kagari_mir::{
    debug::MirCapturedBindingDebugInfo,
    function::{MirFunction, MirParameter},
    instruction::{Instruction, Terminator},
};
use std::iter;

pub(crate) fn lower_callable<'a>(
    module: &'a AnalyzedModule,
    parent: &hir::Function,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let body = instance.callable.clone().expect("callable adapter");
    let Some(TypeId::Function { params, result }) = callable_signature(&body.interface) else {
        return Err(MirLoweringError::MissingBinding(
            "callable adapter signature",
        ));
    };
    let typed = TypedFunction {
        generic_params: vec![],
        bounds: Default::default(),
        id: parent.id,
        name: "$callable".into(),
        params: Default::default(),
        return_type: *result,
    };
    let mut lowerer = FunctionLowerer::new(module, parent, &typed, instance, planner)?;
    lowerer.function.name = typed.name;
    lowerer.function.debug.source_span = body.span;
    let mut values = Vec::new();
    for (index, ty) in iter::once(&body.receiver).chain(params.iter()).enumerate() {
        let physical = lowerer.value_type(ty)?;
        let name = if index == 0 {
            "receiver".into()
        } else {
            format!("arg_{}", index - 1)
        };
        let local = lowerer.alloc_local(name.clone(), physical, body.span);
        lowerer
            .function
            .debug
            .locals
            .last_mut()
            .unwrap()
            .is_parameter = true;
        lowerer.function.params.push(MirParameter {
            name,
            ty: physical,
            local,
        });
        let semantic = lowerer.semantic_type(ty)?;
        lowerer
            .function
            .semantic
            .params
            .insert(index, semantic.clone());
        lowerer
            .function
            .semantic
            .locals
            .insert(local.index(), semantic);
        let value = lowerer.alloc_temp(physical);
        lowerer.emit(Instruction::LoadLocal { dst: value, local });
        values.push(value);
    }
    let result = lowerer.with_debug_span(body.span, |lowerer| {
        let packed = if params.is_empty() {
            lowerer.lower_unit()
        } else {
            let dst = lowerer.alloc_temp(ValueType::HeapObject);
            lowerer.emit(Instruction::MakeTuple {
                dst,
                elements: values[1..].iter().copied().collect(),
            });
            dst
        };
        lowerer.lower_applied_operator(
            body.interface,
            body.receiver,
            &StandardTrait::Fn.contract().methods[0].id,
            &[values[0], packed],
        )
    })?;
    lowerer.set_terminator(if result.ty == ValueType::Never {
        Terminator::Unreachable
    } else {
        Terminator::Return(Some(result))
    });
    lowerer.planner.check()?;
    lowerer.finish()
}

pub(crate) fn lower_iterator<'a>(
    module: &'a AnalyzedModule,
    parent: &hir::Function,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let body = instance.iterator.clone().expect("iterator step");
    let typed = TypedFunction {
        generic_params: vec![],
        bounds: Default::default(),
        id: parent.id,
        name: String::new(),
        params: Default::default(),
        return_type: TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![body.output.clone()],
        },
    };
    let mut lowerer = FunctionLowerer::new(module, parent, &typed, instance, planner)?;
    lowerer.function.name = format!("$iterator_{:?}", body.operation);
    lowerer.function.debug.source_span = body.span;
    let mut args = vec![];
    for (index, ty) in body.captures.iter().enumerate() {
        let physical = lowerer.value_type(ty)?;
        let name = format!("capture_{index}");
        let local = lowerer.alloc_local(name.clone(), physical, body.span);
        lowerer
            .function
            .debug
            .locals
            .last_mut()
            .unwrap()
            .is_parameter = true;
        lowerer.function.params.push(MirParameter {
            name,
            ty: physical,
            local,
        });
        let semantic = lowerer.semantic_type(ty)?;
        lowerer
            .function
            .semantic
            .params
            .insert(index, semantic.clone());
        lowerer
            .function
            .semantic
            .locals
            .insert(local.index(), semantic);
        let value = lowerer.alloc_temp(physical);
        lowerer.emit(Instruction::LoadLocal { dst: value, local });
        args.push(value);
    }
    lowerer.with_debug_span(body.span, |lowerer| {
        lowerer.lower_iterator_step(&body, &args)
    })?;
    lowerer.planner.check()?;
    lowerer.finish()
}

pub(crate) fn lower_protocol<'a>(
    module: &'a AnalyzedModule,
    parent: &hir::Function,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let (protocol, receiver) = instance.protocol.clone().expect("protocol instance");
    let equality = protocol == StandardTrait::PartialEq;
    let typed = TypedFunction {
        generic_params: Vec::new(),
        bounds: Default::default(),
        id: parent.id,
        name: String::new(),
        params: Default::default(),
        return_type: TypeId::Builtin(if equality {
            BuiltinType::Bool
        } else {
            BuiltinType::I64
        }),
    };
    let mut lowerer = FunctionLowerer::new(module, parent, &typed, instance, planner)?;
    lowerer.function.name = format!(
        "$derived_{}_{}",
        protocol.name(),
        lowerer.function.id.index()
    );
    let mut args = Vec::new();
    for index in 0..if equality { 2 } else { 1 } {
        let physical = lowerer.value_type(&receiver)?;
        let name = format!("arg_{index}");
        let local = lowerer.alloc_local(name.clone(), physical, lowerer.function.debug.source_span);
        lowerer
            .function
            .debug
            .locals
            .last_mut()
            .expect("protocol parameter")
            .is_parameter = true;
        lowerer.function.params.push(MirParameter {
            name,
            ty: physical,
            local,
        });
        let semantic = lowerer.semantic_type(&receiver)?;
        lowerer
            .function
            .semantic
            .params
            .insert(index, semantic.clone());
        lowerer
            .function
            .semantic
            .locals
            .insert(local.index(), semantic);
        let value = lowerer.alloc_temp(physical);
        lowerer.emit(Instruction::LoadLocal { dst: value, local });
        args.push(value);
    }
    let value = lowerer.lower_protocol_body(protocol, &receiver, &args, 0)?;
    lowerer.set_terminator(Terminator::Return(Some(value)));
    lowerer.planner.check()?;
    lowerer.finish()
}

pub(crate) fn lower_function<'a>(
    module: &'a AnalyzedModule,
    function: &hir::Function,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let typed = module
        .typed
        .functions
        .iter()
        .find(|typed| typed.id == function.id)
        .ok_or(MirLoweringError::MissingTypedFunction(function.id))?;

    let mut lowerer = FunctionLowerer::new(module, function, typed, instance, planner)?;
    let tail = lowerer.lower_block(function.body)?;
    if !lowerer.current_block_terminated() {
        let value = match tail {
            Some(temp) => Some(temp),
            None => Some(lowerer.lower_unit()),
        };
        lowerer.set_terminator(Terminator::Return(value));
    }

    lowerer.planner.check()?;
    lowerer.finish()
}

pub(crate) fn lower_closure<'a>(
    module: &'a AnalyzedModule,
    parent: &hir::Function,
    closure: hir::ExprId,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let ExprKind::Closure { params, body } = &module.lowered.module.expr(closure).kind else {
        return Err(MirLoweringError::MissingBinding("closure body"));
    };
    let TypeId::Function { result, .. } = module
        .typed
        .type_table
        .expr_type(closure)
        .ok_or(MirLoweringError::MissingExprType(closure))?
    else {
        return Err(MirLoweringError::MissingBinding("closure type"));
    };
    let typed = TypedFunction {
        generic_params: Vec::new(),
        bounds: Default::default(),
        id: parent.id,
        name: format!("closure_{}", closure.index()),
        params: Default::default(),
        return_type: *result,
    };
    let mut lowerer = FunctionLowerer::new(module, parent, &typed, instance, planner)?;
    lowerer.function.name = typed.name;
    lowerer.function.debug.source_span = module.lowered.source_map.expr_span(closure);
    for capture in module.names.closure_captures(closure) {
        let ty = match capture {
            ResolvedName::Local(local) => module
                .typed
                .type_table
                .local_type(*local)
                .ok_or(MirLoweringError::MissingLocalType(*local))?,
            ResolvedName::Param(id) => module
                .typed
                .functions
                .iter()
                .find(|function| function.id == parent.id)
                .and_then(|function| function.params.iter().find(|param| param.id == *id))
                .map(|param| param.ty.clone())
                .ok_or(MirLoweringError::MissingBinding("captured parameter type"))?,
            _ => return Err(MirLoweringError::MissingBinding("closure capture")),
        };
        let name = format!("capture_{}", lowerer.function.params.len());
        let physical = if matches!(capture, ResolvedName::Local(id) if lowerer.cell_locals.contains(id))
        {
            ValueType::HeapObject
        } else {
            lowerer.value_type(&ty)?
        };
        let local = lowerer.alloc_local(
            name.clone(),
            physical,
            module.lowered.source_map.expr_span(closure),
        );
        lowerer
            .function
            .debug
            .locals
            .last_mut()
            .expect("capture local")
            .is_parameter = true;
        let semantic = lowerer.semantic_type(&ty)?;
        lowerer
            .function
            .semantic
            .params
            .insert(lowerer.function.params.len(), semantic.clone());
        lowerer
            .function
            .semantic
            .locals
            .insert(local.index(), semantic);
        lowerer.function.params.push(MirParameter {
            name,
            ty: physical,
            local,
        });
        let capture_name = module
            .names
            .scopes()
            .iter()
            .flat_map(|scope| &scope.bindings)
            .find(|binding| binding.resolved == *capture)
            .map_or_else(|| "<capture>".to_owned(), |binding| binding.name.clone());
        let capture_span = match capture {
            ResolvedName::Local(id) => module.lowered.source_map.local_span(*id),
            ResolvedName::Param(id) => module.lowered.source_map.param_span(*id),
            _ => unreachable!(),
        };
        lowerer
            .function
            .debug
            .captured_bindings
            .push(MirCapturedBindingDebugInfo {
                name: capture_name,
                span: capture_span,
                ty: lowerer.value_type(&ty)?,
            });
        match capture {
            ResolvedName::Local(id) => {
                lowerer.locals.insert(*id, local);
            }
            ResolvedName::Param(id) => {
                lowerer.params.insert(*id, local);
            }
            _ => unreachable!(),
        }
    }
    for param in params {
        let ty = module
            .typed
            .type_table
            .local_type(param.local)
            .ok_or(MirLoweringError::MissingLocalType(param.local))?;
        let value_type = lowerer.value_type(&ty)?;
        let local = lowerer.alloc_local(
            param.name.clone(),
            value_type,
            module.lowered.source_map.local_span(param.local),
        );
        lowerer
            .function
            .debug
            .locals
            .last_mut()
            .expect("closure parameter")
            .is_parameter = true;
        let semantic = lowerer.semantic_type(&ty)?;
        lowerer
            .function
            .semantic
            .params
            .insert(lowerer.function.params.len(), semantic.clone());
        lowerer
            .function
            .semantic
            .locals
            .insert(local.index(), semantic);
        lowerer.function.params.push(MirParameter {
            name: param.name.clone(),
            ty: value_type,
            local,
        });
        lowerer.locals.insert(param.local, local);
    }
    let value = lowerer.lower_expr(*body)?;
    if !lowerer.current_block_terminated() {
        lowerer.set_terminator(Terminator::Return(Some(value)));
    }
    lowerer.planner.check()?;
    lowerer.finish()
}

pub(crate) fn lower_native_method<'a>(
    module: &'a AnalyzedModule,
    parent: &hir::Function,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let (receiver, interface, method) = instance.native_method.clone().expect("native method");
    let contract = planner
        .catalog
        .trait_(&interface.declaration)
        .ok_or(MirLoweringError::MissingBinding("native trait"))?;
    let signature = planner
        .catalog
        .trait_method(&method)
        .ok_or(MirLoweringError::MissingBinding("native method"))?;
    let substitution: TypeSubstitution = contract
        .generic_params
        .iter()
        .cloned()
        .zip(interface.arguments.iter().cloned())
        .collect();
    let instantiate = |ty: &TypeId| {
        ty.with_self(&contract.id, &receiver)
            .instantiate(&substitution)
            .with_associated_types(&interface)
    };
    let params: Vec<_> = signature
        .params
        .iter()
        .map(|p| (p.name.clone(), instantiate(&p.ty)))
        .collect();
    let typed = TypedFunction {
        generic_params: vec![],
        bounds: Default::default(),
        id: parent.id,
        name: String::new(),
        params: Default::default(),
        return_type: instantiate(&signature.return_type),
    };
    let mut lowerer = FunctionLowerer::new(module, parent, &typed, instance, planner)?;
    lowerer.function.name = format!("$native_{}", lowerer.function.id.index());
    let mut args = Vec::new();
    for (index, (name, ty)) in params.iter().enumerate() {
        let physical = lowerer.value_type(ty)?;
        let local = lowerer.alloc_local(name.clone(), physical, lowerer.function.debug.source_span);
        lowerer
            .function
            .debug
            .locals
            .last_mut()
            .unwrap()
            .is_parameter = true;
        lowerer.function.params.push(MirParameter {
            name: name.clone(),
            ty: physical,
            local,
        });
        let semantic = lowerer.semantic_type(ty)?;
        lowerer
            .function
            .semantic
            .params
            .insert(index, semantic.clone());
        lowerer
            .function
            .semantic
            .locals
            .insert(local.index(), semantic);
        let value = lowerer.alloc_temp(physical);
        lowerer.emit(Instruction::LoadLocal { dst: value, local });
        args.push(value);
    }
    let value = lowerer.lower_applied_operator(interface, receiver, &method, &args)?;
    lowerer.set_terminator(if value.ty == ValueType::Never {
        Terminator::Unreachable
    } else {
        Terminator::Return(Some(value))
    });
    lowerer.planner.check()?;
    lowerer.finish()
}

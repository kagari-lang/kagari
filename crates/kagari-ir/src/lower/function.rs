use kagari_hir::AnalyzedModule;
use kagari_hir::hir;

use crate::lower::IrLoweringError;
use crate::lower::state::FunctionLowerer;
use crate::module::function::{IrCapturedBindingDebugInfo, IrParameter};
use crate::module::{function::IrFunction, instruction::Terminator};
use kagari_hir::resolver::ResolvedName;

pub(crate) fn lower_protocol<'a>(
    module: &'a AnalyzedModule,
    parent: &hir::Function,
    instance: super::instances::Instance,
    planner: &mut super::instances::InstancePlanner<'a>,
) -> Result<IrFunction, IrLoweringError> {
    use crate::module::instruction::Instruction;
    let (protocol, receiver) = instance.protocol.clone().expect("protocol instance");
    let equality = protocol == kagari_hir::builtin::traits::StandardTrait::PartialEq;
    let typed = kagari_hir::typeck::TypedFunction {
        generic_params: Vec::new(),
        bounds: Default::default(),
        id: parent.id,
        name: String::new(),
        params: Default::default(),
        return_type: kagari_hir::types::TypeId::Builtin(if equality {
            kagari_hir::types::BuiltinType::Bool
        } else {
            kagari_hir::types::BuiltinType::I64
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
        lowerer.function.params.push(IrParameter {
            name,
            ty: physical,
            local,
        });
        let value = lowerer.alloc_temp(physical);
        lowerer.emit(Instruction::LoadLocal { dst: value, local });
        args.push(value);
    }
    let value = lowerer.lower_protocol_body(protocol, &receiver, &args, 0)?;
    lowerer.set_terminator(Terminator::Return(Some(value)));
    lowerer.planner.check()?;
    Ok(lowerer.finish())
}

pub(crate) fn lower_function<'a>(
    module: &'a AnalyzedModule,
    function: &hir::Function,
    instance: super::instances::Instance,
    planner: &mut super::instances::InstancePlanner<'a>,
) -> Result<IrFunction, IrLoweringError> {
    let typed = module
        .typed
        .functions
        .iter()
        .find(|typed| typed.id == function.id)
        .ok_or(IrLoweringError::MissingTypedFunction(function.id))?;

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
    Ok(lowerer.finish())
}

pub(crate) fn lower_closure<'a>(
    module: &'a AnalyzedModule,
    parent: &hir::Function,
    closure: hir::ExprId,
    instance: super::instances::Instance,
    planner: &mut super::instances::InstancePlanner<'a>,
) -> Result<IrFunction, IrLoweringError> {
    let hir::ExprKind::Closure { params, body } = &module.lowered.module.expr(closure).kind else {
        return Err(IrLoweringError::MissingBinding("closure body"));
    };
    let kagari_hir::types::TypeId::Function { result, .. } = module
        .typed
        .type_table
        .expr_type(closure)
        .ok_or(IrLoweringError::MissingExprType(closure))?
    else {
        return Err(IrLoweringError::MissingBinding("closure type"));
    };
    let typed = kagari_hir::typeck::TypedFunction {
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
                .ok_or(IrLoweringError::MissingLocalType(*local))?,
            ResolvedName::Param(id) => module
                .typed
                .functions
                .iter()
                .find(|function| function.id == parent.id)
                .and_then(|function| function.params.iter().find(|param| param.id == *id))
                .map(|param| param.ty.clone())
                .ok_or(IrLoweringError::MissingBinding("captured parameter type"))?,
            _ => return Err(IrLoweringError::MissingBinding("closure capture")),
        };
        let name = format!("capture_{}", lowerer.function.params.len());
        let physical = if matches!(capture, ResolvedName::Local(id) if lowerer.cell_locals.contains(id))
        {
            crate::module::ValueType::HeapObject
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
        lowerer.function.params.push(IrParameter {
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
            .push(IrCapturedBindingDebugInfo {
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
            .ok_or(IrLoweringError::MissingLocalType(param.local))?;
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
        lowerer.function.params.push(IrParameter {
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
    Ok(lowerer.finish())
}

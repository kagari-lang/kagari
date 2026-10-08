use crate::source::lower::{
    MirLoweringError,
    instances::{Instance, InstancePlanner},
    state::FunctionLowerer,
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, DefinitionPathSegment};
use kagari_contract::standard::RuntimePrimitive;
use kagari_hir::{
    AnalyzedModule,
    hir::{expr::ExprKind, ids::ExprId, item::function::Function},
    language::semantics::{ProtocolSemantics, callable_signature, ordering_type},
    resolver::resolved::ResolvedName,
    typeck::{FunctionImplementation, TypedFunction},
    types::{
        TypeId,
        semantic::{lower_nominal_type, lower_type},
    },
};
use kagari_mir::{
    debug::MirCapturedBindingDebugInfo,
    function::{MirFunction, MirParameter},
    instruction::{Instruction, Terminator},
};
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    language::{Protocol, identity},
    scalar::BuiltinType,
};
use std::iter;

fn completed_output(future: &TypeId) -> Result<TypeId, MirLoweringError> {
    let TypeId::NativeObject(nominal) = future else {
        return Err(MirLoweringError::MissingBinding("checked Future result"));
    };
    nominal
        .arguments
        .first()
        .cloned()
        .ok_or(MirLoweringError::MissingBinding("checked Future output"))
}

pub(crate) fn lower_callable<'a>(
    module: &'a AnalyzedModule,
    parent: &Function,
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
        implementation: FunctionImplementation::Script,
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
            &lowerer.protocol_method(Protocol::Fn, 0)?,
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

pub(crate) fn lower_protocol<'a>(
    module: &'a AnalyzedModule,
    parent: &Function,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let (protocol, receiver) = instance.protocol.clone().expect("protocol instance");
    let binary = matches!(
        protocol,
        Protocol::PartialEq | Protocol::Ord | Protocol::PartialOrd | Protocol::Fn
    );
    let interface = match instance.key.arguments.get(1) {
        Some(TypeId::Trait(interface)) => interface.clone(),
        _ => protocol.nominal(),
    };
    let iteration_result = if protocol.iteration() || protocol == Protocol::Fn {
        let method = planner
            .catalog
            .trait_(&interface.declaration)
            .and_then(|contract| contract.methods.first())
            .ok_or(MirLoweringError::MissingBinding(
                "iteration adapter contract",
            ))?;
        Some(
            planner.catalog.normalize_type(
                &method
                    .return_type
                    .with_self(&method.owner, &receiver)
                    .with_associated_types(&interface),
            ),
        )
    } else {
        None
    };
    let typed = TypedFunction {
        implementation: FunctionImplementation::Script,
        generic_params: Vec::new(),
        bounds: Default::default(),
        id: parent.id,
        name: String::new(),
        params: Default::default(),
        return_type: match protocol {
            Protocol::Iterable | Protocol::Iterator | Protocol::Fn => {
                iteration_result.expect("protocol result")
            }
            Protocol::From => receiver.clone(),
            Protocol::PartialOrd => ordering_type(true),
            Protocol::Ord => ordering_type(false),
            _ => TypeId::Builtin(match protocol {
                Protocol::PartialEq => BuiltinType::Bool,
                Protocol::Hash => BuiltinType::I64,
                Protocol::Debug | Protocol::Display => BuiltinType::String,
                _ => return Err(MirLoweringError::MissingBinding("closed protocol adapter")),
            }),
        },
    };

    let mut lowerer = FunctionLowerer::new(module, parent, &typed, instance, planner)?;
    let mut member = identity(protocol);
    member.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: match protocol {
            Protocol::Iterable => "iter",
            Protocol::Iterator => "next",
            Protocol::PartialEq => "eq",
            Protocol::Ord => "cmp",
            Protocol::PartialOrd => "partial_cmp",
            Protocol::Fn => "call",
            Protocol::From => "from",
            Protocol::Hash => "hash",
            Protocol::Debug => "debug",
            Protocol::Display => "display",
            _ => unreachable!("checked adapter kind"),
        }
        .into(),
        occurrence: 0,
    });
    let required = NativeCallableRequirement {
        receiver: lower_type(&receiver),
        interface: lower_nominal_type(&interface),
        member,
        arguments: vec![],
    };
    // Direct derived equality also works without an installed protocol contract.
    // Only a checked, declared implicit application can be selected by native code.
    if lowerer
        .planner
        .catalog
        .implicit_protocol_application(
            &required,
            &receiver,
            &interface,
            &lowerer.planner.options.cancel,
        )
        .map_err(|_| MirLoweringError::MissingBinding("checked protocol adapter"))?
        .is_some()
    {
        lowerer.function.semantic.protocol_adapter = Some(required.clone());
    }
    lowerer.function.name = format!(
        "$derived_{}_{}",
        protocol.name(),
        lowerer.function.id.index()
    );
    let mut args = Vec::new();
    for index in 0..if binary { 2 } else { 1 } {
        let parameter = if protocol == Protocol::From || (protocol == Protocol::Fn && index == 1) {
            interface
                .arguments
                .first()
                .ok_or(MirLoweringError::MissingBinding("callable argument tuple"))?
        } else {
            &receiver
        };
        let physical = lowerer.value_type(parameter)?;
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
        let semantic = lowerer.semantic_type(parameter)?;
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
    let value = match protocol {
        Protocol::Iterable | Protocol::Iterator | Protocol::Fn | Protocol::From => {
            if protocol == Protocol::Iterable && typed.return_type == receiver {
                args[0]
            } else {
                lowerer.lower_applied_operator(
                    interface,
                    receiver.clone(),
                    &required.member,
                    &args,
                )?
            }
        }
        Protocol::PartialOrd => lowerer.emit_intrinsic(
            RuntimePrimitive::ValuePartialCmp,
            &args,
            ValueType::HeapObject,
        ),
        Protocol::Ord => {
            lowerer.emit_intrinsic(RuntimePrimitive::ValueCmp, &args, ValueType::HeapObject)
        }
        Protocol::Debug | Protocol::Display => lowerer.emit_intrinsic(
            if protocol == Protocol::Debug {
                RuntimePrimitive::ValueDebug
            } else {
                RuntimePrimitive::ValueDisplay
            },
            &args,
            ValueType::Str,
        ),
        _ => lowerer.lower_protocol_body(protocol, &receiver, &args, 0)?,
    };
    lowerer.set_terminator(Terminator::Return(Some(value)));
    lowerer.planner.check()?;
    lowerer.finish()
}

pub(crate) fn lower_function<'a>(
    module: &'a AnalyzedModule,
    function: &Function,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let typed = module
        .typed
        .functions
        .iter()
        .find(|typed| typed.id == function.id)
        .ok_or(MirLoweringError::MissingTypedFunction(function.id))?;
    let mut typed = typed.clone();
    if instance.resume {
        typed.return_type = completed_output(&typed.return_type)?;
    }
    let mut lowerer = FunctionLowerer::new(module, function, &typed, instance, planner)?;
    if function.is_async && !lowerer.instance.resume {
        lowerer.lower_future_factory()?;
        return lowerer.finish();
    }
    lowerer.mark_resume();
    let body = function
        .body
        .ok_or(MirLoweringError::MissingBinding("script function body"))?;
    let tail = lowerer.lower_block(body)?;
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
    parent: &Function,
    closure: ExprId,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let ExprKind::Closure {
        params,
        body,
        is_async,
    } = &module.lowered.module.expr(closure).kind
    else {
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
        implementation: FunctionImplementation::Script,
        generic_params: Vec::new(),
        bounds: Default::default(),
        id: parent.id,
        name: format!("closure_{}", closure.index()),
        params: Default::default(),
        return_type: if instance.resume {
            completed_output(&result)?
        } else {
            *result
        },
    };
    let mut lowerer = FunctionLowerer::new(module, parent, &typed, instance, planner)?;
    lowerer.function.name = typed.name;
    lowerer.mark_resume();
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
    if *is_async && !lowerer.instance.resume {
        lowerer.lower_future_factory()?;
        return lowerer.finish();
    }
    let value = lowerer.lower_expr(*body)?;
    if !lowerer.current_block_terminated() {
        lowerer.set_terminator(Terminator::Return(Some(value)));
    }
    lowerer.planner.check()?;
    lowerer.finish()
}

use crate::source::elaborate::{build::Builder, elaborate};
use crate::source::lower::{
    MirLoweringError,
    instances::{Instance, InstancePlanner},
    state::{FunctionLowerer, LoweringSignature},
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, mapping::DefinitionMappingError};
use kagari_contract::standard::RuntimePrimitive;
use kagari_hir::{
    compile_input::{
        body::{BodyRoot, bindings::BindingId},
        program::CheckedModule,
    },
    hir::{ids::ExprId, item::function::Function},
    language::semantics::{ProtocolSemantics, callable_signature, ordering_type},
    types::TypeId,
};
use kagari_mir::{
    debug::MirCapturedBindingDebugInfo,
    function::{MirFunction, MirParameter},
    instruction::{Instruction, Terminator},
};
use kagari_types::{
    declaration::requirement::NativeCallableRequirement, language::Protocol, scalar::BuiltinType,
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
    module: &'a CheckedModule<'a>,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let body = instance.callable.clone().expect("callable adapter");
    let Some(TypeId::Function { params, result }) =
        callable_signature(&planner.definitions(), &body.interface)
    else {
        return Err(MirLoweringError::MissingBinding(
            "callable adapter signature",
        ));
    };
    let typed = LoweringSignature {
        bounds: Default::default(),
        name: "$callable".into(),
        params: Default::default(),
        return_type: *result,
    };
    let mut lowerer = FunctionLowerer::new(module, &typed, None, instance, planner)?;
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
            .insert(local.index(), semantic.clone());
        let value = lowerer.alloc_temp(physical);
        lowerer
            .function
            .semantic
            .registers
            .insert(value.temp.index(), semantic);
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
    module: &'a CheckedModule<'a>,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let (protocol, receiver) = instance.protocol.clone().expect("protocol instance");
    let arithmetic = matches!(
        protocol,
        Protocol::Neg
            | Protocol::Add
            | Protocol::Sub
            | Protocol::Mul
            | Protocol::Div
            | Protocol::Rem
    );
    let binary = matches!(
        protocol,
        Protocol::PartialEq | Protocol::Ord | Protocol::PartialOrd | Protocol::Fn
    ) || (arithmetic && protocol.binary_operator());
    // Arithmetic markers must retain the selected application, including an
    // omitted Output binding, rather than reconstruct a stronger intrinsic view.
    let interface = match instance.key.arguments.get(1) {
        Some(TypeId::Trait(interface)) => interface.clone(),
        _ if arithmetic => {
            return Err(MirLoweringError::MissingBinding(
                "primitive arithmetic adapter interface",
            ));
        }
        _ => protocol
            .nominal(&planner.definitions())
            .expect("registered protocol identity"),
    };
    let iteration_result = if protocol.iteration() || protocol == Protocol::Fn {
        Some(planner.protocol_adapter_result(&interface, &receiver)?)
    } else {
        None
    };
    let generic_params = match &receiver {
        TypeId::Tuple(members) if !receiver.is_concrete() => members
            .iter()
            .filter_map(|ty| {
                let TypeId::Generic(parameter) = ty else {
                    return None;
                };
                Some(parameter.clone())
            })
            .collect::<Vec<_>>(),
        _ => vec![],
    };
    let bounds = generic_params
        .iter()
        .map(|parameter| (TypeId::Generic(parameter.clone()), vec![interface.clone()]))
        .collect();
    let typed = LoweringSignature {
        bounds,
        name: String::new(),
        params: Default::default(),
        return_type: match protocol {
            Protocol::Iterable | Protocol::Iterator | Protocol::Fn => {
                iteration_result.expect("protocol result")
            }
            Protocol::Neg
            | Protocol::Add
            | Protocol::Sub
            | Protocol::Mul
            | Protocol::Div
            | Protocol::Rem
            | Protocol::From => receiver.clone(),
            Protocol::PartialOrd => ordering_type(&planner.definitions(), true),
            Protocol::Ord => ordering_type(&planner.definitions(), false),
            _ => TypeId::Builtin(match protocol {
                Protocol::PartialEq => BuiltinType::Bool,
                Protocol::Hash => BuiltinType::I64,
                Protocol::Debug | Protocol::Display => BuiltinType::String,
                _ => return Err(MirLoweringError::MissingBinding("closed protocol adapter")),
            }),
        },
    };

    let mut lowerer = FunctionLowerer::new(module, &typed, None, instance, planner)?;
    let member_name = match protocol {
        Protocol::Iterable => "iter",
        Protocol::Iterator => "next",
        Protocol::PartialEq => "eq",
        Protocol::Ord => "cmp",
        Protocol::PartialOrd => "partial_cmp",
        Protocol::Fn => "call",
        Protocol::From => "from",
        Protocol::Neg => "neg",
        Protocol::Add => "add",
        Protocol::Sub => "sub",
        Protocol::Mul => "mul",
        Protocol::Div => "div",
        Protocol::Rem => "rem",
        Protocol::Hash => "hash",
        Protocol::Debug => "debug",
        Protocol::Display => "display",
        _ => unreachable!("checked adapter kind"),
    };
    let protocol_owner = protocol
        .definition(&lowerer.planner.definitions())
        .expect("registered protocol");
    let member = lowerer
        .planner
        .context
        .intern_child(protocol_owner, DefinitionKind::Method, member_name, 0)
        .map_err(DefinitionMappingError::from)?;
    let required = NativeCallableRequirement {
        receiver: lowerer.planner.encode_type(&receiver),
        interface: lowerer.planner.encode_nominal(&interface),
        member,
        arguments: vec![],
    };
    // Direct derived equality also works without an installed protocol contract.
    // Only a checked, declared implicit application can be selected by native code.
    if lowerer.function.semantic.generic.is_some()
        || lowerer
            .planner
            .has_protocol_application(&required, &receiver, &interface)?
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
            .insert(local.index(), semantic.clone());
        let value = lowerer.alloc_temp(physical);
        lowerer
            .function
            .semantic
            .registers
            .insert(value.temp.index(), semantic);
        lowerer.emit(Instruction::LoadLocal { dst: value, local });
        args.push(value);
    }
    let value = match protocol {
        Protocol::Neg
        | Protocol::Add
        | Protocol::Sub
        | Protocol::Mul
        | Protocol::Div
        | Protocol::Rem
        | Protocol::Iterable
        | Protocol::Iterator
        | Protocol::Fn
        | Protocol::From => {
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
    module: &'a CheckedModule<'a>,
    function: &Function,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let typed = module
        .environment()
        .signatures()
        .functions()
        .iter()
        .find(|typed| typed.id == function.id)
        .ok_or(MirLoweringError::MissingTypedFunction(function.id))?;
    let mut typed = typed.clone();
    if instance.resume {
        typed.return_type = completed_output(&typed.return_type)?;
    }
    let input = module
        .body_for(instance.body.owner())
        .ok_or(MirLoweringError::MissingBinding("admitted function body"))?;
    let is_factory = function.is_async && !instance.resume;
    let body = if is_factory {
        None
    } else {
        Some(elaborate(input, &planner.options.cancel)?)
    };
    let mut lowerer = FunctionLowerer::new(
        module,
        &LoweringSignature::from(&typed),
        body,
        instance,
        planner,
    )?;
    if is_factory {
        lowerer.lower_future_factory()?;
        return lowerer.finish();
    }
    lowerer.mark_resume();
    let BodyRoot::Block(body) = lowerer.source_body().root else {
        return Err(MirLoweringError::MissingBinding("function root block"));
    };
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
    module: &'a CheckedModule<'a>,
    closure: ExprId,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let input = module
        .body_for(instance.body.owner())
        .and_then(|body| body.closure(closure))
        .ok_or(MirLoweringError::MissingBinding("admitted nested callable"))?;
    let builder = Builder::new(input, &planner.options.cancel)?;
    let callable = builder
        .callable()
        .cloned()
        .ok_or(MirLoweringError::MissingBinding("nested callable contract"))?;
    let result = if callable.is_block {
        callable.ty.clone()
    } else if let TypeId::Function { result, .. } = &callable.ty {
        result.as_ref().clone()
    } else {
        return Err(MirLoweringError::MissingBinding("closure type"));
    };
    let typed = LoweringSignature {
        bounds: Default::default(),
        name: format!(
            "{}_{}",
            if callable.is_block {
                "async_block"
            } else {
                "closure"
            },
            closure.index()
        ),
        params: Default::default(),
        return_type: if instance.resume {
            completed_output(&result)?
        } else {
            result
        },
    };
    let captures = builder.captured_bindings().to_vec();
    let BodyRoot::Expression(body) = input.root() else {
        return Err(MirLoweringError::MissingBinding(
            "nested callable expression root",
        ));
    };
    let is_factory = callable.is_async && !instance.resume;
    let expanded = if is_factory {
        None
    } else {
        Some(builder.finish()?)
    };
    let mut lowerer = FunctionLowerer::new(module, &typed, expanded, instance, planner)?;
    lowerer.function.name = typed.name;
    lowerer.mark_resume();
    lowerer.function.debug.source_span = callable.span;
    for capture in captures {
        let name = format!("capture_{}", lowerer.function.params.len());
        let physical = if capture.by_cell {
            ValueType::HeapObject
        } else {
            lowerer.value_type(&capture.ty)?
        };
        let local = lowerer.alloc_local(name.clone(), physical, callable.span);
        lowerer
            .function
            .debug
            .locals
            .last_mut()
            .expect("capture local")
            .is_parameter = true;
        let semantic = lowerer.semantic_type(&capture.ty)?;
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
        lowerer
            .function
            .debug
            .captured_bindings
            .push(MirCapturedBindingDebugInfo {
                name: capture.name,
                span: capture.span,
                ty: lowerer.value_type(&capture.ty)?,
            });
        match capture.id {
            BindingId::Local(id) => {
                lowerer.locals.insert(id, local);
            }
            BindingId::Parameter(id) => {
                lowerer.params.insert(id, local);
            }
        }
    }
    for param in callable.parameters {
        let value_type = lowerer.value_type(&param.ty)?;
        let local = lowerer.alloc_local(param.name.clone(), value_type, param.span);
        lowerer
            .function
            .debug
            .locals
            .last_mut()
            .expect("closure parameter")
            .is_parameter = true;
        let semantic = lowerer.semantic_type(&param.ty)?;
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
            name: param.name,
            ty: value_type,
            local,
        });
        lowerer.locals.insert(param.id, local);
    }
    if is_factory {
        lowerer.lower_future_factory()?;
        return lowerer.finish();
    }
    let value = lowerer.lower_expr(body)?;
    if !lowerer.current_block_terminated() {
        lowerer.set_terminator(Terminator::Return(Some(value)));
    }
    lowerer.planner.check()?;
    lowerer.finish()
}

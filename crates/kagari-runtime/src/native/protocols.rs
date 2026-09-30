//! Invoke carried protocol selections without a source lookup or a nested VM.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    builtin::BuiltinError,
    native::{NativeCallback, NativeCallbackTarget, callback},
    value::Value,
    value_semantics,
};
use kagari_abi::{
    callable::{CallableImplementation, EngineNativeBinding, NativeBinding},
    native_import::{NativeWitness, NativeWitnessImplementation},
    operations::IterOp,
    scalar::BuiltinType,
    standard::{
        StandardIntrinsic,
        bindings::{NativeDefaultMethod, NativeProtocolMethod},
        surface::StandardEnum,
    },
    types::{self as abi, AbiType, PublicAbiItem},
};
use kagari_common::identity::{DefinitionKind, DefinitionPathSegment, associated_type_id};
use std::slice;

pub(super) enum ProtocolStep {
    Value(Value),
    Call(NativeCallback),
    BuiltinFailure(BuiltinError),
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native iterator witness mismatch")
}

fn provider(owner: &LoadedModule, witness: &NativeWitness) -> Option<NativeProtocolMethod> {
    let NativeWitnessImplementation::Table(instance) = &witness.implementation else {
        return None;
    };
    let owner = owner
        .members()
        .find(|module| module.bytecode.identity == instance.declaration.module)?;
    let table = owner
        .bytecode
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) if table.declaration == instance.declaration => {
                Some(table)
            }
            _ => None,
        })?;
    let [method] = table.methods.as_slice() else {
        return None;
    };
    match method.implementation {
        CallableImplementation::Native(NativeBinding::Engine(EngineNativeBinding::Protocol(
            binding,
        ))) => Some(binding),
        _ => None,
    }
}

pub(super) fn parse(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    text: Value,
    output: &AbiType,
) -> Result<ProtocolStep, RuntimeError> {
    let string = AbiType::Builtin(BuiltinType::String);
    if !runtime.matches_interface_method_abi(&text, &string, owner) {
        return Err(invalid());
    }
    if provider(owner, witness) == Some(NativeProtocolMethod::NumericFromStr) {
        let AbiType::Builtin(scalar) = witness.receiver else {
            return Err(invalid());
        };
        return match runtime
            .invoke_standard_builtin(StandardIntrinsic::ParseNumber(scalar), &[text])
        {
            Ok(value) => Ok(ProtocolStep::Value(value)),
            Err(error) => Ok(ProtocolStep::BuiltinFailure(error)),
        };
    }
    table_call(runtime, owner, witness, 0, vec![text], &[string], output)
}

pub(super) fn numeric_destination(
    owner: &LoadedModule,
    witness: &NativeWitness,
    operation: NativeDefaultMethod,
) -> bool {
    provider(owner, witness)
        == Some(if operation == NativeDefaultMethod::Sum {
            NativeProtocolMethod::NumericSum
        } else {
            NativeProtocolMethod::NumericProduct
        })
}

pub(super) fn iter(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    source: Value,
    output: &AbiType,
) -> Result<ProtocolStep, RuntimeError> {
    if !runtime.matches_interface_method_abi(&source, &witness.receiver, owner) {
        return Err(invalid());
    }
    if witness.implementation == NativeWitnessImplementation::Primitive
        && &witness.receiver == output
    {
        return Ok(ProtocolStep::Value(source));
    }
    if provider(owner, witness) == Some(NativeProtocolMethod::CollectionIter) {
        return runtime
            .iter_operation(owner, &source, &witness.receiver, IterOp::New)
            .map(ProtocolStep::Value);
    }
    if witness.implementation == NativeWitnessImplementation::Interface {
        let mut interface = witness.interface.clone();
        interface.associated_types.insert(
            associated_type_id(&interface.declaration, "Iter"),
            output.clone(),
        );
        let method = runtime.resolve_interface_method_slot(&source, &interface, 0)?;
        if method.return_type() != output
            || method.parameter_types() != slice::from_ref(method.concrete_type())
        {
            return Err(invalid());
        }
        let arguments = vec![method.receiver().clone()];
        return Ok(ProtocolStep::Call(NativeCallback {
            target: NativeCallbackTarget::Interface(Box::new(method)),
            arguments,
        }));
    }
    table_call(
        runtime,
        owner,
        witness,
        0,
        vec![source],
        slice::from_ref(&witness.receiver),
        output,
    )
}

pub(super) fn next(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    source: Value,
    output: &AbiType,
) -> Result<ProtocolStep, RuntimeError> {
    if !runtime.matches_interface_method_abi(&source, &witness.receiver, owner) {
        return Err(invalid());
    }
    if matches!(witness.receiver, AbiType::Iter(_)) {
        if let Some(step) = runtime.gc().iter_step(&source, &witness.receiver)? {
            let signature = AbiType::Function {
                params: vec![],
                result: Box::new(output.clone()),
            };
            return callback(runtime, &step, &signature, vec![]).map(ProtocolStep::Call);
        }
        return runtime
            .iter_operation(owner, &source, &witness.receiver, IterOp::Next)
            .map(ProtocolStep::Value);
    }
    table_call(
        runtime,
        owner,
        witness,
        0,
        vec![source],
        slice::from_ref(&witness.receiver),
        output,
    )
}

pub(super) fn compare(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    left: Value,
    right: Value,
) -> Result<ProtocolStep, RuntimeError> {
    if [&left, &right]
        .iter()
        .any(|value| !runtime.matches_interface_method_abi(value, &witness.receiver, owner))
    {
        return Err(invalid());
    }
    let arguments = vec![left, right];
    if witness.implementation == NativeWitnessImplementation::Primitive {
        return runtime
            .invoke_standard_builtin(StandardIntrinsic::ValueCmp, &arguments)
            .map(ProtocolStep::Value)
            .map_err(|error| error.into_runtime_error());
    }
    let output = AbiType::StandardEnum {
        kind: StandardEnum::Ordering,
        args: vec![],
    };
    table_call(
        runtime,
        owner,
        witness,
        0,
        arguments,
        &[witness.receiver.clone(), witness.receiver.clone()],
        &output,
    )
}

pub(super) fn equal(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    left: Value,
    right: Value,
) -> Result<ProtocolStep, RuntimeError> {
    if [&left, &right]
        .iter()
        .any(|value| !runtime.matches_interface_method_abi(value, &witness.receiver, owner))
    {
        return Err(invalid());
    }
    if matches!(
        witness.implementation,
        NativeWitnessImplementation::Primitive | NativeWitnessImplementation::Interface
    ) {
        return value_semantics::script_equal(runtime.gc(), &left, &right)
            .map(|equal| ProtocolStep::Value(Value::Bool(equal)));
    }
    let arguments = vec![left, right];
    if witness.implementation == NativeWitnessImplementation::Derived {
        return derived_call(
            runtime,
            owner,
            witness,
            arguments,
            &[witness.receiver.clone(), witness.receiver.clone()],
            &AbiType::Builtin(BuiltinType::Bool),
        );
    }
    table_call(
        runtime,
        owner,
        witness,
        0,
        arguments,
        &[witness.receiver.clone(), witness.receiver.clone()],
        &AbiType::Builtin(BuiltinType::Bool),
    )
}

fn derived_call(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    arguments: Vec<Value>,
    params: &[AbiType],
    result: &AbiType,
) -> Result<ProtocolStep, RuntimeError> {
    let [target] = witness.methods.as_slice() else {
        return Err(invalid());
    };
    let implementation = owner
        .members()
        .find(|module| module.bytecode.identity == target.declaration.module)
        .ok_or_else(invalid)?;
    let function = implementation
        .bytecode
        .functions
        .iter()
        .find(|function| function.identity.as_ref() == Some(target))
        .ok_or_else(invalid)?;
    let metadata = &function.metadata;
    if metadata.params.len() != params.len()
        || params
            .iter()
            .enumerate()
            .any(|(slot, ty)| metadata.semantic.params.get(&slot) != Some(ty))
        || metadata.semantic.result.as_ref() != Some(result)
    {
        return Err(invalid());
    }
    let function = function.id;
    runtime.validate_loaded_module(&implementation)?;
    Ok(ProtocolStep::Call(NativeCallback {
        target: NativeCallbackTarget::Function {
            implementation,
            function,
        },
        arguments,
    }))
}
pub(super) fn hash(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    value: Value,
) -> Result<ProtocolStep, RuntimeError> {
    if !runtime.matches_interface_method_abi(&value, &witness.receiver, owner) {
        return Err(invalid());
    }
    if matches!(
        witness.implementation,
        NativeWitnessImplementation::Primitive | NativeWitnessImplementation::Interface
    ) {
        return runtime
            .invoke_standard_builtin(StandardIntrinsic::ValueHash, &[value])
            .map(ProtocolStep::Value)
            .map_err(|error| error.into_runtime_error());
    }
    let arguments = vec![value];
    let result = AbiType::Builtin(BuiltinType::I64);
    if witness.implementation == NativeWitnessImplementation::Derived {
        return derived_call(
            runtime,
            owner,
            witness,
            arguments,
            slice::from_ref(&witness.receiver),
            &result,
        );
    }
    table_call(
        runtime,
        owner,
        witness,
        0,
        arguments,
        slice::from_ref(&witness.receiver),
        &result,
    )
}

pub(super) fn aggregate(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    source: Value,
    source_type: &AbiType,
) -> Result<NativeCallback, RuntimeError> {
    match table_call(
        runtime,
        owner,
        witness,
        0,
        vec![source],
        slice::from_ref(source_type),
        &witness.receiver,
    )? {
        ProtocolStep::Call(request) => Ok(request),
        ProtocolStep::Value(_) | ProtocolStep::BuiltinFailure(_) => Err(invalid()),
    }
}

/// Required List ordinals are len, is_empty and get; the validated trait
/// declaration supplies their actual dynamic slots and canonical identities.
pub(super) fn list(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    required_slot: usize,
    arguments: Vec<Value>,
    output: &AbiType,
) -> Result<ProtocolStep, RuntimeError> {
    if required_slot > 2
        || arguments.first().is_none_or(|value| {
            !runtime.matches_interface_method_abi(value, &witness.receiver, owner)
        })
    {
        return Err(invalid());
    }
    if witness.implementation == NativeWitnessImplementation::Interface {
        return interface_call(runtime, owner, witness, required_slot, arguments, output);
    }
    // Native storage has no generated script methods. Its selected table is
    // checked against these exact storage bindings by the portable linker.
    if matches!(witness.receiver, AbiType::Array(_, _))
        && matches!(
            witness.implementation,
            NativeWitnessImplementation::Table(_)
        )
        && witness.methods.is_empty()
    {
        let binding = [
            StandardIntrinsic::ArrayLen,
            StandardIntrinsic::ArrayIsEmpty,
            StandardIntrinsic::ArrayGet,
        ][required_slot];
        return match runtime.invoke_standard_builtin(binding, &arguments) {
            Ok(value) => Ok(ProtocolStep::Value(value)),
            Err(error) => Ok(ProtocolStep::BuiltinFailure(error)),
        };
    }
    let mut parameters = vec![witness.receiver.clone()];
    if required_slot == 2 {
        parameters.push(AbiType::Builtin(BuiltinType::USize));
    }
    table_call(
        runtime,
        owner,
        witness,
        required_slot,
        arguments,
        &parameters,
        output,
    )
}

pub(super) fn set_contains(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    source: Value,
    item: Value,
) -> Result<ProtocolStep, RuntimeError> {
    let [ty] = witness.interface.arguments.as_slice() else {
        return Err(invalid());
    };
    if !runtime.matches_interface_method_abi(&source, &witness.receiver, owner)
        || !runtime.matches_interface_method_abi(&item, ty, owner)
    {
        return Err(invalid());
    }
    let arguments = vec![source, item];
    let output = AbiType::Builtin(BuiltinType::Bool);
    if witness.implementation == NativeWitnessImplementation::Interface {
        return interface_call(runtime, owner, witness, 2, arguments, &output);
    }
    table_call(
        runtime,
        owner,
        witness,
        2,
        arguments,
        &[witness.receiver.clone(), ty.clone()],
        &output,
    )
}
fn interface_call(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    required_slot: usize,
    arguments: Vec<Value>,
    output: &AbiType,
) -> Result<ProtocolStep, RuntimeError> {
    let declaration = owner
        .members()
        .find(|module| module.bytecode.identity == witness.interface.declaration.module)
        .and_then(|module| {
            abi::trait_contract(
                &module.bytecode.identity,
                &module.bytecode.public_items,
                &module.bytecode.trait_contracts,
                &witness.interface.declaration,
            )
            .cloned()
        })
        .ok_or_else(invalid)?;
    let (slot, _) = declaration
        .methods
        .iter()
        .enumerate()
        .filter(|(_, method)| method.implementation == CallableImplementation::Required)
        .nth(required_slot)
        .ok_or_else(invalid)?;
    let method = runtime.resolve_interface_method_slot(&arguments[0], &witness.interface, slot)?;
    let mut arguments = arguments;
    arguments[0] = method.receiver().clone();
    if method.return_type() != output {
        return Err(invalid());
    }
    runtime.validate_interface_method_arguments(&method, &arguments)?;
    Ok(ProtocolStep::Call(NativeCallback {
        target: NativeCallbackTarget::Interface(Box::new(method)),
        arguments,
    }))
}

fn table_call(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    required_slot: usize,
    arguments: Vec<Value>,
    parameters: &[AbiType],
    output: &AbiType,
) -> Result<ProtocolStep, RuntimeError> {
    match &witness.implementation {
        NativeWitnessImplementation::Table(instance) => {
            let implementation = owner
                .members()
                .find(|module| module.bytecode.identity == instance.declaration.module)
                .ok_or_else(invalid)?;
            let table = implementation
                .bytecode
                .public_items
                .iter()
                .find_map(|item| match item {
                    PublicAbiItem::InterfaceTable(table)
                        if table.declaration == instance.declaration =>
                    {
                        table.instantiate(&instance.arguments)
                    }
                    _ => None,
                })
                .ok_or_else(invalid)?;
            if table.for_type != witness.receiver {
                return Err(invalid());
            }
            let declared = owner
                .members()
                .find(|module| module.bytecode.identity == witness.interface.declaration.module)
                .and_then(|module| {
                    abi::trait_contract(
                        &module.bytecode.identity,
                        &module.bytecode.public_items,
                        &module.bytecode.trait_contracts,
                        &witness.interface.declaration,
                    )
                    .cloned()
                })
                .and_then(|contract| {
                    contract
                        .methods
                        .into_iter()
                        .filter(|method| method.implementation == CallableImplementation::Required)
                        .nth(required_slot)
                })
                .ok_or_else(invalid)?;
            let method = table
                .methods
                .iter()
                .find(|method| method.name == declared.name)
                .ok_or_else(invalid)?;
            let mut declaration = instance.declaration.clone();
            declaration.path.push(DefinitionPathSegment {
                kind: DefinitionKind::Method,
                name: method.name.clone(),
                occurrence: 0,
            });
            let target = witness
                .methods
                .iter()
                .find(|target| target.declaration == declaration)
                .ok_or_else(invalid)?;
            let function = implementation
                .bytecode
                .functions
                .iter()
                .find(|function| function.identity.as_ref() == Some(target))
                .ok_or_else(invalid)?;
            let function = function.id;
            runtime.validate_loaded_module(&implementation)?;
            let metadata = &implementation.bytecode.functions[function.index()].metadata;
            if metadata.params.len() != arguments.len()
                || parameters.len() != arguments.len()
                || arguments
                    .iter()
                    .zip(parameters)
                    .enumerate()
                    .any(|(slot, (value, ty))| {
                        metadata.semantic.params.get(&slot) != Some(ty)
                            || !runtime.matches_interface_method_abi(value, ty, owner)
                    })
                || metadata.semantic.result.as_ref() != Some(output)
            {
                return Err(invalid());
            }
            Ok(ProtocolStep::Call(NativeCallback {
                target: NativeCallbackTarget::Function {
                    implementation,
                    function,
                },
                arguments,
            }))
        }
        NativeWitnessImplementation::Interface
        | NativeWitnessImplementation::Host
        | NativeWitnessImplementation::Primitive
        | NativeWitnessImplementation::Derived => Err(invalid()),
    }
}

/// RangeBounds remains a static protocol: native ranges or selected script methods.
pub(super) fn range_bound(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    source: Value,
    upper: bool,
) -> Result<ProtocolStep, RuntimeError> {
    let output = AbiType::StandardEnum {
        kind: StandardEnum::Bound,
        args: vec![AbiType::Builtin(BuiltinType::USize)],
    };
    if !runtime.matches_interface_method_abi(&source, &witness.receiver, owner) {
        return Err(invalid());
    }
    if witness.methods.is_empty()
        && matches!(
            witness.implementation,
            NativeWitnessImplementation::Table(_)
        )
    {
        let Value::Range(range) = source else {
            return Err(invalid());
        };
        return range
            .bound(runtime.gc(), &witness.receiver, &output, upper)
            .map(ProtocolStep::Value);
    }
    table_call(
        runtime,
        owner,
        witness,
        usize::from(upper),
        vec![source],
        slice::from_ref(&witness.receiver),
        &output,
    )
}

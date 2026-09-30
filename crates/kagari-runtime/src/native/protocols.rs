//! Invoke carried protocol selections without a source lookup or a nested VM.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    native::{NativeCallback, NativeCallbackTarget, callback},
    value::Value,
};
use kagari_abi::{
    callable::{CallableImplementation, EngineNativeBinding, NativeBinding},
    native_import::{NativeWitness, NativeWitnessImplementation},
    operations::IterOp,
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
        arguments,
        &[witness.receiver.clone(), witness.receiver.clone()],
        &output,
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
        vec![source],
        slice::from_ref(source_type),
        &witness.receiver,
    )? {
        ProtocolStep::Call(request) => Ok(request),
        ProtocolStep::Value(_) => Err(invalid()),
    }
}

fn table_call(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
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
                .and_then(|contract| contract.methods.into_iter().next())
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
            let [target] = witness.methods.as_slice() else {
                return Err(invalid());
            };
            if target.declaration != declaration {
                return Err(invalid());
            }
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
        | NativeWitnessImplementation::Primitive => Err(invalid()),
    }
}

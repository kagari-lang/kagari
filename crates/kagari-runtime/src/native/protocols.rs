//! Invoke carried protocol selections without a source lookup or a nested VM.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    native::{NativeCallback, NativeCallbackTarget, callback},
    value::Value,
};
use kagari_abi::{
    native_import::{NativeWitness, NativeWitnessImplementation},
    operations::IterOp,
    standard::{StandardIntrinsic, surface::StandardEnum},
    types::{self as abi, AbiType, PublicAbiItem},
};
use kagari_common::identity::{DefinitionKind, DefinitionPathSegment};

pub(super) enum ProtocolStep {
    Value(Value),
    Call(NativeCallback),
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native iterator witness mismatch")
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
    table_call(runtime, owner, witness, vec![source], output)
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
    table_call(runtime, owner, witness, arguments, &output)
}

fn table_call(
    runtime: &Runtime,
    owner: &LoadedModule,
    witness: &NativeWitness,
    arguments: Vec<Value>,
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
            let function = implementation
                .bytecode
                .functions
                .iter()
                .find(|function| {
                    function.identity.as_ref().is_some_and(|identity| {
                        identity.declaration == declaration
                            && identity.arguments == instance.arguments
                    })
                })
                .ok_or_else(invalid)?;
            let function = function.id;
            runtime.validate_loaded_module(&implementation)?;
            let metadata = &implementation.bytecode.functions[function.index()].metadata;
            if metadata.params.len() != arguments.len()
                || arguments.iter().enumerate().any(|(slot, value)| {
                    metadata.semantic.params.get(&slot) != Some(&witness.receiver)
                        || !runtime.matches_interface_method_abi(value, &witness.receiver, owner)
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

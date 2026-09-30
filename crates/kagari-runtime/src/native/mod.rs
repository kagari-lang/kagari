//! Bounded native method state retained by the caller's execution frame.
mod enums;
mod iterators;
mod protocols;
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{ClosureValueSnapshot, RootSet},
    native::{
        enums::{EnumInvocation, SCRATCH_ROOTS},
        iterators::IteratorInvocation,
    },
    value::Value,
};
use kagari_abi::{
    callable::EngineNativeBinding, ids::FunctionRef, native_import::EngineNativeOperation,
    types::AbiType,
};
use kagari_bytecode::{EngineImportId, Register};

/// A checked callback request. Its callable and arguments stay rooted by the
/// suspended native invocation until the callback's frame has been entered.
pub struct NativeCallback {
    pub(crate) target: NativeCallbackTarget,
    pub(crate) arguments: Vec<Value>,
}

pub(crate) enum NativeCallbackTarget {
    Closure(ClosureValueSnapshot),
    Function {
        implementation: LoadedModule,
        function: FunctionRef,
    },
}

/// Driver actions contain no library policy. Each advance is one charged logical
/// operation; callbacks use the same frame stack as ordinary script calls.
pub enum NativeProgress {
    Continue,
    Callback(NativeCallback),
    Finished,
}

pub(crate) enum NativeAction {
    Continue,
    Callback(NativeCallback),
    Publish(Value),
    Finish,
    Complete(Value),
}

enum NativeState {
    Enum(EnumInvocation),
    Iterator(IteratorInvocation),
}

pub(crate) struct NativeInvocation {
    pub(crate) destination: Option<Register>,
    implementation: LoadedModule,
    import: EngineImportId,
    roots: RootSet,
    state: NativeState,
}

impl NativeInvocation {
    pub(crate) fn start(
        runtime: &Runtime,
        implementation: LoadedModule,
        import: EngineImportId,
        arguments: &[Value],
        destination: Option<Register>,
    ) -> Result<Self, RuntimeError> {
        let contract = implementation
            .bytecode
            .engine_imports
            .get(import.index())
            .ok_or_else(|| RuntimeError::module_validation("invalid native continuation import"))?;
        if arguments.len() != contract.signature.params.len() {
            return Err(RuntimeError::module_validation(
                "native continuation argument count",
            ));
        }
        let mut state = match implementation.engine_binding(import) {
            Some(EngineNativeOperation::Resumable(EngineNativeBinding::Intrinsic(operation))) => {
                NativeState::Enum(EnumInvocation::start(
                    runtime,
                    operation,
                    &contract.signature,
                    arguments,
                )?)
            }
            Some(EngineNativeOperation::Resumable(EngineNativeBinding::TraitDefault(
                operation,
            ))) => NativeState::Iterator(IteratorInvocation::start(
                runtime, operation, contract, arguments,
            )?),
            _ => {
                return Err(RuntimeError::module_validation(
                    "invalid native continuation binding",
                ));
            }
        };
        let roots = runtime
            .gc()
            .root_execution_values(
                arguments
                    .iter()
                    .cloned()
                    .chain(match &mut state {
                        NativeState::Enum(_) => vec![Value::Unit; SCRATCH_ROOTS],
                        NativeState::Iterator(state) => {
                            vec![
                                state.initial.take().ok_or_else(|| {
                                    RuntimeError::module_validation(
                                        "missing terminal initial value",
                                    )
                                })?,
                                Value::Unit,
                                Value::Unit,
                            ]
                        }
                    })
                    .collect(),
            )
            .ok_or_else(|| {
                RuntimeError::module_validation("invalid native continuation argument")
            })?;
        Ok(Self {
            implementation,
            import,
            destination,
            roots,
            state,
        })
    }

    pub(crate) fn advance(&mut self, runtime: &Runtime) -> Result<NativeAction, RuntimeError> {
        let contract = &self.implementation.bytecode.engine_imports[self.import.index()];
        match &mut self.state {
            NativeState::Enum(state) => state.advance(
                runtime,
                &self.implementation,
                &contract.signature,
                &self.roots,
            ),
            NativeState::Iterator(state) => {
                state.advance(runtime, &self.implementation, contract, &self.roots)
            }
        }
    }

    pub(crate) fn receive(&mut self, runtime: &Runtime, value: Value) -> Result<(), RuntimeError> {
        match &mut self.state {
            NativeState::Enum(state) => state.receive(runtime, &self.roots, value),
            NativeState::Iterator(state) => state.receive(runtime, &self.roots, value),
        }
    }
}

fn callback(
    runtime: &Runtime,
    value: &Value,
    signature: &AbiType,
    arguments: Vec<Value>,
) -> Result<NativeCallback, RuntimeError> {
    let AbiType::Function { params, result } = signature else {
        return Err(RuntimeError::module_validation("native callback signature"));
    };
    let closure = runtime.resolve_closure(value)?;
    let function = &closure.implementation.bytecode.functions[closure.function.index()];
    let prefix = closure.captures.len();
    if function.metadata.params.len() != prefix + params.len()
        || function.metadata.semantic.result.as_ref() != Some(result.as_ref())
        || params.iter().enumerate().any(|(slot, expected)| {
            function.metadata.semantic.params.get(&(prefix + slot)) != Some(expected)
        })
        || arguments.len() != params.len()
        || arguments.iter().zip(params).any(|(value, ty)| {
            !runtime.matches_interface_method_abi(value, ty, &closure.implementation)
        })
    {
        return Err(RuntimeError::module_validation(
            "native callback contract mismatch",
        ));
    }
    Ok(NativeCallback {
        target: NativeCallbackTarget::Closure(closure),
        arguments,
    })
}

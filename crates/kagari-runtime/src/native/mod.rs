//! Bounded native method state retained by the caller's execution frame.
mod array_copy;
mod array_initialization;
mod array_ranges;
mod enums;
mod iterators;
mod keys;
mod list_equality;
mod lists;
mod map_snapshots;
mod prepared_arrays;
mod protocols;
mod results;
mod retention;
use crate::{
    LoadedModule, RootedInterfaceMethod, Runtime, RuntimeError,
    builtin::BuiltinError,
    gc::{ClosureValueSnapshot, RootSet},
    native::{
        array_copy::ArrayCopy,
        array_initialization::ArrayInitialization,
        array_ranges::ArrayRange,
        enums::{EnumInvocation, SCRATCH_ROOTS},
        iterators::IteratorInvocation,
        keys::KeyInvocation,
        list_equality::EqualityInvocation,
        lists::ListInvocation,
        map_snapshots::SnapshotInvocation,
        prepared_arrays::PreparedArray,
        retention::Retention,
    },
    value::Value,
};
use kagari_abi::{
    callable::EngineNativeBinding,
    ids::FunctionRef,
    native_import::EngineNativeOperation,
    standard::{
        StandardIntrinsic,
        bindings::{NativeDefaultMethod, NativeProtocolMethod},
        traits::StandardTrait,
    },
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
    Interface(Box<RootedInterfaceMethod>),
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
    BuiltinFailure(BuiltinError),
    TypeMismatch(&'static str),
}

pub(crate) enum NativeAction {
    Continue,
    Callback(NativeCallback),
    Publish(Value),
    Finish,
    Complete(Value),
    BuiltinFailure(BuiltinError),
    TypeMismatch(&'static str),
}

enum NativeState {
    Key(KeyInvocation),
    PreparedArray(Box<PreparedArray>),
    Retention(Retention),
    ArrayRange(ArrayRange),
    ArrayCopy(ArrayCopy),
    ArrayInitialization(ArrayInitialization),
    Enum(EnumInvocation),
    Iterator(IteratorInvocation),
    List(ListInvocation),
    ListEquality(EqualityInvocation),
    MapSnapshot(SnapshotInvocation),
    Forward,
}

pub(crate) struct NativeInvocation {
    pub(crate) destination: Option<Register>,
    implementation: LoadedModule,
    import: EngineImportId,
    roots: RootSet,
    state: NativeState,
    entry: Option<NativeAction>,
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
        let mut entry = None;
        let mut state = match implementation.engine_binding(import) {
            Some(EngineNativeOperation::Resumable(binding))
                if matches!(
                    binding,
                    EngineNativeBinding::Intrinsic(
                        StandardIntrinsic::LinkedHashMapFrom | StandardIntrinsic::LinkedHashSetFrom
                    ) | EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionFromIterator)
                ) && matches!(
                    contract.signature.result,
                    AbiType::Map { .. } | AbiType::Set(..)
                ) =>
            {
                NativeState::Key(KeyInvocation::construct(contract, arguments)?)
            }

            Some(EngineNativeOperation::Resumable(EngineNativeBinding::Intrinsic(
                operation @ (StandardIntrinsic::MapGet
                | StandardIntrinsic::MapContainsKey
                | StandardIntrinsic::MapInsert
                | StandardIntrinsic::MapRemove
                | StandardIntrinsic::SetContains
                | StandardIntrinsic::SetInsert
                | StandardIntrinsic::SetRemove
                | StandardIntrinsic::MapGetOrInsertWith
                | StandardIntrinsic::MapUpdate),
            ))) => NativeState::Key(KeyInvocation::start(operation, contract, arguments)?),
            Some(EngineNativeOperation::Resumable(EngineNativeBinding::Intrinsic(
                operation @ (StandardIntrinsic::ArraySort
                | StandardIntrinsic::ArraySortBy
                | StandardIntrinsic::ArraySortByKey
                | StandardIntrinsic::ArrayDedup),
            ))) => {
                NativeState::PreparedArray(Box::new(PreparedArray::start(operation, arguments)?))
            }
            Some(EngineNativeOperation::Resumable(EngineNativeBinding::Intrinsic(
                StandardIntrinsic::ArrayRetain
                | StandardIntrinsic::MapRetain
                | StandardIntrinsic::SetRetain,
            ))) => NativeState::Retention(Retention::start(&contract.signature, arguments)?),
            Some(EngineNativeOperation::Resumable(EngineNativeBinding::Intrinsic(
                StandardIntrinsic::ArrayCopyWithin | StandardIntrinsic::ArrayRemoveRange,
            ))) => NativeState::ArrayRange(ArrayRange::start(contract, arguments)),
            Some(EngineNativeOperation::Resumable(
                EngineNativeBinding::Intrinsic(
                    StandardIntrinsic::ArrayListFrom
                    | StandardIntrinsic::ArrayCopyFrom
                    | StandardIntrinsic::ArrayExtend,
                )
                | EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionFromIterator),
            )) => NativeState::ArrayCopy(ArrayCopy::start(contract, arguments)?),
            Some(EngineNativeOperation::Resumable(EngineNativeBinding::Intrinsic(
                StandardIntrinsic::ArrayListFromFn,
            ))) => NativeState::ArrayInitialization(ArrayInitialization::start(arguments)?),
            Some(EngineNativeOperation::Resumable(
                EngineNativeBinding::Intrinsic(
                    StandardIntrinsic::MapKeys
                    | StandardIntrinsic::MapValues
                    | StandardIntrinsic::MapEntries,
                )
                | EngineNativeBinding::TraitDefault(
                    NativeDefaultMethod::MapKeysView
                    | NativeDefaultMethod::MapValuesView
                    | NativeDefaultMethod::MapEntriesView,
                ),
            )) => NativeState::MapSnapshot(SnapshotInvocation::start(contract, arguments)?),
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
            ))) => {
                if matches!(
                    operation,
                    NativeDefaultMethod::SetUnion
                        | NativeDefaultMethod::SetIntersection
                        | NativeDefaultMethod::SetDifference
                        | NativeDefaultMethod::SetSymmetricDifference
                        | NativeDefaultMethod::SetIsSubset
                        | NativeDefaultMethod::SetIsSuperset
                        | NativeDefaultMethod::SetIsDisjoint
                ) {
                    NativeState::Key(KeyInvocation::sets(operation, arguments))
                } else if matches!(
                    operation,
                    NativeDefaultMethod::ListContains
                        | NativeDefaultMethod::ListStartsWith
                        | NativeDefaultMethod::ListEndsWith
                ) {
                    NativeState::ListEquality(EqualityInvocation::start(operation, arguments)?)
                } else if matches!(
                    operation,
                    NativeDefaultMethod::ListFirst
                        | NativeDefaultMethod::ListLast
                        | NativeDefaultMethod::ListBinarySearch
                ) {
                    NativeState::List(ListInvocation::start(operation, arguments)?)
                } else {
                    let target = if matches!(
                        operation,
                        NativeDefaultMethod::Sum | NativeDefaultMethod::Product
                    ) {
                        Some(
                            contract
                                .witnesses
                                .iter()
                                .find(|witness| {
                                    witness.receiver == contract.signature.result
                                        && StandardTrait::from_id(&witness.interface.declaration)
                                            == Some(if operation == NativeDefaultMethod::Sum {
                                                StandardTrait::Sum
                                            } else {
                                                StandardTrait::Product
                                            })
                                })
                                .ok_or_else(|| {
                                    RuntimeError::module_validation("missing aggregation witness")
                                })?,
                        )
                    } else {
                        None
                    };
                    if let Some(witness) = target.filter(|witness| {
                        !protocols::numeric_destination(&implementation, witness, operation)
                    }) {
                        entry = Some(NativeAction::Callback(protocols::aggregate(
                            runtime,
                            &implementation,
                            witness,
                            arguments[0].clone(),
                            &contract.signature.params[0],
                        )?));
                        NativeState::Forward
                    } else {
                        NativeState::Iterator(IteratorInvocation::start(
                            runtime, operation, contract, arguments,
                        )?)
                    }
                }
            }
            Some(EngineNativeOperation::Resumable(EngineNativeBinding::Protocol(
                operation @ (NativeProtocolMethod::NumericSum
                | NativeProtocolMethod::NumericProduct),
            ))) => NativeState::Iterator(IteratorInvocation::start(
                runtime,
                if operation == NativeProtocolMethod::NumericSum {
                    NativeDefaultMethod::Sum
                } else {
                    NativeDefaultMethod::Product
                },
                contract,
                arguments,
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
                        NativeState::Key(state) => vec![Value::Unit; state.scratch_roots()],
                        NativeState::Enum(_) => vec![Value::Unit; SCRATCH_ROOTS],
                        NativeState::PreparedArray(_) => {
                            vec![Value::Unit; prepared_arrays::SCRATCH_ROOTS]
                        }
                        NativeState::Retention(_) => vec![Value::Unit; retention::SCRATCH_ROOTS],
                        NativeState::Iterator(state) => state.initial.take().ok_or_else(|| {
                            RuntimeError::module_validation("missing terminal initial roots")
                        })?,
                        NativeState::List(state) => state.initial.take().ok_or_else(|| {
                            RuntimeError::module_validation("missing List initial roots")
                        })?,
                        NativeState::ListEquality(state) => {
                            state.initial.take().ok_or_else(|| {
                                RuntimeError::module_validation(
                                    "missing List equality initial roots",
                                )
                            })?
                        }
                        NativeState::Forward => vec![],
                        NativeState::ArrayInitialization(_) => {
                            vec![Value::Unit; array_initialization::SCRATCH_ROOTS]
                        }
                        NativeState::ArrayCopy(_) => vec![Value::Unit; array_copy::SCRATCH_ROOTS],
                        NativeState::ArrayRange(_) => {
                            vec![Value::Unit; array_ranges::SCRATCH_ROOTS]
                        }
                        NativeState::MapSnapshot(state) => {
                            state.initial.take().ok_or_else(|| {
                                RuntimeError::module_validation(
                                    "missing Map snapshot initial roots",
                                )
                            })?
                        }
                    })
                    .collect(),
            )
            .ok_or_else(|| {
                RuntimeError::module_validation("invalid native continuation argument")
            })?;
        if let NativeState::MapSnapshot(state) = &state {
            entry = state
                .initialize(runtime, arguments, &roots)?
                .map(NativeAction::BuiltinFailure);
        }
        if let NativeState::ArrayInitialization(state) = &state {
            entry = state
                .initialize(runtime, &roots)?
                .map(NativeAction::BuiltinFailure);
        }
        if let NativeState::PreparedArray(state) = &mut state {
            state.initialize(runtime, &roots)?;
        }
        if let NativeState::Retention(state) = &mut state {
            state.initialize(runtime, &roots)?;
        }
        let initialized = match &mut state {
            NativeState::Key(state) => {
                Some(state.initialize(runtime, &implementation, contract, &roots)?)
            }
            NativeState::ArrayRange(state) => {
                Some(state.initialize(runtime, &implementation, contract, &roots)?)
            }
            NativeState::ArrayCopy(state) => {
                Some(state.initialize(runtime, &implementation, contract, &roots)?)
            }
            _ => None,
        };
        if let Some(action) = initialized {
            entry = Some(action);
        }
        Ok(Self {
            implementation,
            import,
            destination,
            roots,
            state,
            entry,
        })
    }

    pub(crate) fn take_entry(&mut self) -> NativeAction {
        self.entry.take().unwrap_or(NativeAction::Continue)
    }

    pub(crate) fn advance(&mut self, runtime: &Runtime) -> Result<NativeAction, RuntimeError> {
        let contract = &self.implementation.bytecode.engine_imports[self.import.index()];
        match &mut self.state {
            NativeState::Key(state) => {
                state.advance(runtime, &self.implementation, contract, &self.roots)
            }
            NativeState::PreparedArray(state) => {
                state.advance(runtime, &self.implementation, contract, &self.roots)
            }
            NativeState::Retention(state) => state.advance(
                runtime,
                &self.implementation,
                &contract.signature,
                &self.roots,
            ),
            NativeState::ArrayRange(state) => {
                state.advance(runtime, &self.implementation, contract, &self.roots)
            }
            NativeState::ArrayCopy(state) => {
                state.advance(runtime, &self.implementation, contract, &self.roots)
            }
            NativeState::ArrayInitialization(state) => {
                state.advance(runtime, &contract.signature, &self.roots)
            }
            NativeState::Enum(state) => state.advance(
                runtime,
                &self.implementation,
                &contract.signature,
                &self.roots,
            ),
            NativeState::Iterator(state) => {
                state.advance(runtime, &self.implementation, contract, &self.roots)
            }
            NativeState::List(state) => {
                state.advance(runtime, &self.implementation, contract, &self.roots)
            }
            NativeState::ListEquality(state) => {
                state.advance(runtime, &self.implementation, contract, &self.roots)
            }
            NativeState::Forward => Err(RuntimeError::module_validation(
                "aggregation callback is pending",
            )),
            NativeState::MapSnapshot(state) => {
                state.advance(runtime, &self.implementation, contract, &self.roots)
            }
        }
    }

    pub(crate) fn receive(
        &mut self,
        runtime: &Runtime,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        match &mut self.state {
            NativeState::Key(state) => state.receive(
                runtime,
                &self.implementation,
                &self.implementation.bytecode.engine_imports[self.import.index()],
                &self.roots,
                value,
            ),
            NativeState::PreparedArray(state) => state.receive(
                runtime,
                &self.implementation,
                &self.implementation.bytecode.engine_imports[self.import.index()],
                &self.roots,
                value,
            ),
            NativeState::Retention(state) => {
                state.receive(runtime, &self.implementation, &self.roots, value)
            }
            NativeState::ArrayRange(state) => state.receive(
                runtime,
                &self.implementation,
                &self.implementation.bytecode.engine_imports[self.import.index()],
                &self.roots,
                value,
            ),
            NativeState::ArrayCopy(state) => state.receive(
                runtime,
                &self.implementation,
                &self.implementation.bytecode.engine_imports[self.import.index()],
                &self.roots,
                value,
            ),
            NativeState::ArrayInitialization(state) => state.receive(
                runtime,
                &self.implementation,
                &self.implementation.bytecode.engine_imports[self.import.index()].signature,
                &self.roots,
                value,
            ),
            NativeState::Enum(state) => state
                .receive(runtime, &self.roots, value)
                .map(|()| NativeAction::Continue),
            NativeState::Iterator(state) => state
                .receive(
                    runtime,
                    &self.implementation,
                    &self.implementation.bytecode.engine_imports[self.import.index()],
                    &self.roots,
                    value,
                )
                .map(|()| NativeAction::Continue),
            NativeState::List(state) => state.receive(
                runtime,
                &self.implementation,
                &self.implementation.bytecode.engine_imports[self.import.index()],
                &self.roots,
                value,
            ),
            NativeState::ListEquality(state) => state.receive(
                runtime,
                &self.implementation,
                &self.implementation.bytecode.engine_imports[self.import.index()],
                &self.roots,
                value,
            ),
            NativeState::Forward => {
                let contract = &self.implementation.bytecode.engine_imports[self.import.index()];
                if !runtime.matches_interface_method_abi(
                    &value,
                    &contract.signature.result,
                    &self.implementation,
                ) {
                    return Err(RuntimeError::module_validation(
                        "aggregation callback result mismatch",
                    ));
                }
                Ok(NativeAction::Complete(value))
            }
            NativeState::MapSnapshot(state) => state.receive(
                runtime,
                &self.implementation,
                &self.implementation.bytecode.engine_imports[self.import.index()],
                &self.roots,
                value,
            ),
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

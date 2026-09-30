//! Once-only native construction; no source advancement occurs here.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::RootSet,
    native::{
        NativeAction,
        lazy_iterators::source_list,
        protocols::{self, ProtocolStep},
        sources::{IteratorSelection, SourceSelection},
    },
    value::{EnumTag, Value},
};
use kagari_abi::{
    native_import::EngineNativeImport,
    scalar::BuiltinType,
    standard::{StandardIntrinsic, bindings::NativeDefaultMethod},
    types::AbiType,
};
use kagari_bytecode::EngineImportId;
use kagari_common::identity::associated_type_id;

pub(super) const SCRATCH: usize = 6;
const STATE: usize = 0;
const FLAG: usize = 1;
const OTHER: usize = 2;
const LENGTH: usize = 3;
const CAPTURE: usize = 4;
const INITIAL: usize = 5;
#[derive(Clone, Copy)]
enum Phase {
    SizeZero,
    SizeCompare,
    Message,
    Assert,
    Convert,
    WaitingConvert,
    Len,
    WaitingLen,
    Initial,
    State,
    FlagInitial,
    Flag,
    Capture,
    Dependencies,
    Iter,
}
pub(super) struct LazyConstructor {
    operation: NativeDefaultMethod,
    phase: Phase,
    scratch: usize,
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native lazy construction contract mismatch")
}
impl LazyConstructor {
    pub(super) fn start(operation: NativeDefaultMethod, arguments: &[Value]) -> Self {
        let phase = match operation {
            NativeDefaultMethod::ListWindows | NativeDefaultMethod::ListChunks => Phase::SizeZero,
            NativeDefaultMethod::Zip | NativeDefaultMethod::Chain => Phase::Convert,
            NativeDefaultMethod::Map
            | NativeDefaultMethod::Filter
            | NativeDefaultMethod::FilterMap
            | NativeDefaultMethod::Inspect => Phase::Capture,
            NativeDefaultMethod::Take | NativeDefaultMethod::Skip => Phase::State,
            _ => Phase::Initial,
        };
        Self {
            operation,
            phase,
            scratch: arguments.len(),
        }
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        import: EngineImportId,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        let get = |slot| roots.get(self.scratch + slot).ok_or_else(invalid);
        let set = |slot, value| {
            roots
                .set(runtime.gc(), self.scratch + slot, value)
                .ok_or_else(invalid)
        };
        match self.phase {
            Phase::SizeZero => self.phase = Phase::SizeCompare,
            Phase::SizeCompare => self.phase = Phase::Message,
            Phase::Message => self.phase = Phase::Assert,
            Phase::Assert => {
                let Value::U64(size) = roots.get(1).ok_or_else(invalid)? else {
                    return Err(invalid());
                };
                if let Err(error) = runtime.invoke_standard_builtin(
                    StandardIntrinsic::DebugAssert,
                    &[
                        Value::Bool(size != 0),
                        Value::Str("window or chunk size must be nonzero".into()),
                    ],
                ) {
                    return Ok(NativeAction::BuiltinFailure(error));
                }
                self.phase = Phase::Convert;
            }
            Phase::Convert => {
                let slot = usize::from(matches!(
                    self.operation,
                    NativeDefaultMethod::Zip | NativeDefaultMethod::Chain
                ));
                let source = &contract.signature.params[slot];
                let selection = SourceSelection::select(contract, source, None, slot)?;
                let witness = selection.iterable(contract);
                let iterator = witness
                    .interface
                    .associated_types
                    .get(&associated_type_id(&witness.interface.declaration, "Iter"))
                    .ok_or_else(invalid)?;
                self.phase = Phase::WaitingConvert;
                return self.converted(
                    runtime,
                    roots,
                    protocols::iter(
                        runtime,
                        owner,
                        witness,
                        roots.get(slot).ok_or_else(invalid)?,
                        iterator,
                    )?,
                );
            }
            Phase::Len => {
                let witness = source_list(owner, contract)?;
                self.phase = Phase::WaitingLen;
                return self.converted(
                    runtime,
                    roots,
                    protocols::list(
                        runtime,
                        owner,
                        witness,
                        0,
                        vec![roots.get(0).ok_or_else(invalid)?],
                        &AbiType::Builtin(BuiltinType::USize),
                    )?,
                );
            }
            Phase::Initial => {
                let value = match self.operation {
                    NativeDefaultMethod::FlatMap | NativeDefaultMethod::Flatten => {
                        Value::Enum(runtime.alloc_enum(EnumTag::OptionNone, vec![])?)
                    }
                    NativeDefaultMethod::Enumerate => Value::U64(0),
                    NativeDefaultMethod::Chain => Value::I32(0),
                    _ => Value::Bool(false),
                };
                set(INITIAL, value)?;
                self.phase = Phase::State;
            }
            Phase::State => {
                let value = if matches!(
                    self.operation,
                    NativeDefaultMethod::Take | NativeDefaultMethod::Skip
                ) {
                    roots.get(1).ok_or_else(invalid)?
                } else if self.windows() {
                    Value::U64(0)
                } else {
                    get(INITIAL)?
                };
                set(STATE, Value::Array(runtime.gc().alloc_array(vec![value])?))?;
                self.phase = if matches!(
                    self.operation,
                    NativeDefaultMethod::FlatMap | NativeDefaultMethod::Flatten
                ) {
                    Phase::FlagInitial
                } else {
                    Phase::Capture
                };
            }
            Phase::FlagInitial => {
                set(INITIAL, Value::Bool(false))?;
                self.phase = Phase::Flag;
            }
            Phase::Flag => {
                set(
                    FLAG,
                    Value::Array(runtime.gc().alloc_array(vec![get(INITIAL)?])?),
                )?;
                self.phase = Phase::Capture;
            }
            Phase::Capture => {
                let source = roots.get(0).ok_or_else(invalid)?;
                let captures = match self.operation {
                    NativeDefaultMethod::FlatMap => vec![
                        source,
                        get(STATE)?,
                        get(FLAG)?,
                        roots.get(1).ok_or_else(invalid)?,
                    ],
                    NativeDefaultMethod::Flatten => vec![source, get(STATE)?, get(FLAG)?],
                    NativeDefaultMethod::Fuse
                    | NativeDefaultMethod::Take
                    | NativeDefaultMethod::Skip
                    | NativeDefaultMethod::Enumerate => vec![source, get(STATE)?],
                    NativeDefaultMethod::TakeWhile | NativeDefaultMethod::SkipWhile => {
                        vec![source, roots.get(1).ok_or_else(invalid)?, get(STATE)?]
                    }
                    NativeDefaultMethod::Zip => vec![source, get(OTHER)?],
                    NativeDefaultMethod::Chain => vec![source, get(OTHER)?, get(STATE)?],
                    NativeDefaultMethod::ListWindows | NativeDefaultMethod::ListChunks => vec![
                        source,
                        roots.get(1).ok_or_else(invalid)?,
                        get(STATE)?,
                        get(OTHER)?,
                        get(LENGTH)?,
                    ],
                    _ => vec![source, roots.get(1).ok_or_else(invalid)?],
                };
                let retention = runtime
                    .modules
                    .retain_runtime_program(owner)
                    .ok_or_else(invalid)?;
                set(
                    CAPTURE,
                    runtime
                        .gc()
                        .alloc_iterator_capture(owner, import, captures, retention)?,
                )?;
                self.phase = Phase::Dependencies;
            }
            Phase::Dependencies => self.phase = Phase::Iter,
            Phase::Iter => {
                let AbiType::Iter(item) = &contract.signature.result else {
                    return Err(invalid());
                };
                let source = roots.get(0).ok_or_else(invalid)?;
                let dependencies = match self.operation {
                    NativeDefaultMethod::ListWindows | NativeDefaultMethod::ListChunks => {
                        vec![get(OTHER)?]
                    }
                    NativeDefaultMethod::Zip | NativeDefaultMethod::Chain => {
                        vec![source, get(OTHER)?]
                    }
                    NativeDefaultMethod::FlatMap | NativeDefaultMethod::Flatten => {
                        let inner = if self.operation == NativeDefaultMethod::FlatMap {
                            let AbiType::Function { result, .. } = &contract.signature.params[1]
                            else {
                                return Err(invalid());
                            };
                            result.as_ref()
                        } else {
                            IteratorSelection::select(contract, &contract.signature.params[0])?
                                .item(contract)?
                        };
                        let selection = SourceSelection::select(contract, inner, None, 0)?;
                        if matches!(selection.next(contract).receiver, AbiType::Iter(_)) {
                            vec![source, get(STATE)?]
                        } else {
                            vec![source]
                        }
                    }
                    _ => vec![source],
                };
                let retention = runtime
                    .modules
                    .retain_runtime_program(owner)
                    .ok_or_else(invalid)?;
                return runtime
                    .gc()
                    .new_lazy_iter(
                        get(CAPTURE)?,
                        dependencies,
                        item.as_ref().clone(),
                        owner,
                        retention,
                    )
                    .map(NativeAction::Complete);
            }
            Phase::WaitingConvert | Phase::WaitingLen => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
    fn windows(&self) -> bool {
        matches!(
            self.operation,
            NativeDefaultMethod::ListWindows | NativeDefaultMethod::ListChunks
        )
    }
    fn converted(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
        progress: ProtocolStep,
    ) -> Result<NativeAction, RuntimeError> {
        match progress {
            ProtocolStep::Value(value) => self.receive(runtime, roots, value),
            ProtocolStep::Call(request) => Ok(NativeAction::Callback(request)),
            ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
        }
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        let (slot, phase) = match self.phase {
            Phase::WaitingConvert => (
                OTHER,
                if self.windows() {
                    Phase::Len
                } else if self.operation == NativeDefaultMethod::Chain {
                    Phase::Initial
                } else {
                    Phase::Capture
                },
            ),
            Phase::WaitingLen => (LENGTH, Phase::State),
            _ => return Err(invalid()),
        };
        roots
            .set(runtime.gc(), self.scratch + slot, value)
            .ok_or_else(invalid)?;
        self.phase = phase;
        Ok(NativeAction::Continue)
    }
}

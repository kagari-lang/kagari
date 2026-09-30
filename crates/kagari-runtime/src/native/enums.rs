//! Option/Result algorithms, with a safepoint at every original logical operation.
use crate::{
    LoadedModule, Runtime, RuntimeError, RuntimeErrorKind,
    gc::RootSet,
    native::{NativeAction, callback},
    value::{EnumTag, Value},
};
use kagari_abi::{
    native_import::NativeSignature,
    standard::{StandardIntrinsic, surface::StandardEnum},
    types::AbiType,
};

// Scratch roots are independent of the declaration's parameter count.
const PAYLOAD: usize = 0;
const SECOND: usize = 1;
const RESULT: usize = 2;
pub(super) const SCRATCH_ROOTS: usize = 3;

#[derive(Clone, Copy)]
enum AfterCallback {
    Move,
    Wrap(u32),
    PreserveError,
    Filter,
}

#[derive(Clone, Copy)]
enum Phase {
    Branch,
    ReadOuter,
    Invoke {
        slot: usize,
        payload: bool,
        after: AfterCallback,
    },
    Waiting(AfterCallback),
    Wrap(u32),
    PreserveOuterError,
    ConstantFalse,
    InnerTest,
    InnerBranch(u32),
    FilterBranch,
    InnerRead(u32),
    ZipTuple,
    NestedWrap,
    NestedEmpty,
    NestedError,
    PreserveInnerError,
    InnerMove,
    InnerJump,
    Move,
    Jump,
}

pub(super) struct EnumInvocation {
    operation: StandardIntrinsic,
    variant: u32,
    phase: Phase,
    scratch: usize,
    inner: bool,
}

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "native enum contract mismatch",
    )
}

fn family(ty: &AbiType) -> Result<(StandardEnum, &[AbiType]), RuntimeError> {
    match ty {
        AbiType::StandardEnum { kind, args }
            if matches!(kind, StandardEnum::Option | StandardEnum::Result)
                && args.len() == kind.arity() =>
        {
            Ok((*kind, args))
        }
        _ => Err(invalid()),
    }
}

fn variant(runtime: &Runtime, value: &Value, ty: &AbiType) -> Result<u32, RuntimeError> {
    let (kind, _) = family(ty)?;
    let Value::Enum(id) = value else {
        return Err(invalid());
    };
    let snapshot = runtime.gc().enum_snapshot(*id).ok_or_else(invalid)?;
    match (kind, snapshot.tag, snapshot.fields.len()) {
        (StandardEnum::Option, EnumTag::OptionSome, 1)
        | (StandardEnum::Result, EnumTag::ResultOk, 1) => Ok(0),
        (StandardEnum::Option, EnumTag::OptionNone, 0)
        | (StandardEnum::Result, EnumTag::ResultErr, 1) => Ok(1),
        _ => Err(invalid()),
    }
}

fn read(
    runtime: &Runtime,
    value: &Value,
    ty: &AbiType,
    expected: u32,
) -> Result<Value, RuntimeError> {
    if variant(runtime, value, ty)? != expected {
        return Err(invalid());
    }
    let (_, args) = family(ty)?;
    let Value::Enum(id) = value else {
        return Err(invalid());
    };
    let value = runtime
        .gc()
        .enum_snapshot(*id)
        .and_then(|snapshot| snapshot.fields.into_iter().next())
        .ok_or_else(invalid)?;
    if !value.has_representation(
        args.get(expected as usize)
            .ok_or_else(invalid)?
            .representation(),
    ) {
        return Err(invalid());
    }
    Ok(value)
}

fn make(
    runtime: &Runtime,
    ty: &AbiType,
    selected: u32,
    value: Value,
) -> Result<Value, RuntimeError> {
    let (kind, args) = family(ty)?;
    let (tag, payload) = match (kind, selected) {
        (StandardEnum::Option, 0) => (EnumTag::OptionSome, Some(value)),
        (StandardEnum::Option, 1) => (EnumTag::OptionNone, None),
        (StandardEnum::Result, 0) => (EnumTag::ResultOk, Some(value)),
        (StandardEnum::Result, 1) => (EnumTag::ResultErr, Some(value)),
        _ => return Err(invalid()),
    };
    if payload
        .as_ref()
        .is_some_and(|value| !value.has_representation(args[selected as usize].representation()))
    {
        return Err(invalid());
    }
    runtime
        .alloc_enum(tag, payload.into_iter().collect())
        .map(Value::Enum)
}

impl EnumInvocation {
    pub(super) fn start(
        runtime: &Runtime,
        operation: StandardIntrinsic,
        signature: &NativeSignature,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        // Entry consumes the former outer Test charge. Dispatch/selection below
        // only manipulates rooted state; it never hides another logical operation.
        Ok(Self {
            operation,
            variant: variant(runtime, &arguments[0], &signature.params[0])?,
            phase: Phase::Branch,
            scratch: arguments.len(),
            inner: false,
        })
    }

    fn get(&self, roots: &RootSet, slot: usize) -> Result<Value, RuntimeError> {
        roots.get(self.scratch + slot).ok_or_else(invalid)
    }
    fn set(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
        slot: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        roots
            .set(runtime.gc(), self.scratch + slot, value)
            .ok_or_else(invalid)
    }
    fn result(&self, runtime: &Runtime, roots: &RootSet, value: Value) -> Result<(), RuntimeError> {
        self.set(runtime, roots, RESULT, value)
    }
    fn move_phase(&self) -> Phase {
        if self.inner {
            Phase::InnerMove
        } else {
            Phase::Move
        }
    }

    fn select(&self, runtime: &Runtime, roots: &RootSet) -> Result<Phase, RuntimeError> {
        let success = self.variant == 0;
        let invoke = |slot, payload, after| Phase::Invoke {
            slot,
            payload,
            after,
        };
        let phase = match self.operation {
            StandardIntrinsic::OptionUnwrapOrElse | StandardIntrinsic::ResultUnwrapOrElse => {
                if success {
                    Phase::Move
                } else {
                    invoke(
                        1,
                        self.operation == StandardIntrinsic::ResultUnwrapOrElse,
                        AfterCallback::Move,
                    )
                }
            }
            StandardIntrinsic::OptionMap | StandardIntrinsic::ResultMap => {
                if success {
                    invoke(1, true, AfterCallback::Wrap(0))
                } else if self.operation == StandardIntrinsic::ResultMap {
                    Phase::PreserveOuterError
                } else {
                    Phase::Wrap(1)
                }
            }
            StandardIntrinsic::OptionAndThen | StandardIntrinsic::ResultAndThen => {
                if success {
                    invoke(1, true, AfterCallback::Move)
                } else if self.operation == StandardIntrinsic::ResultAndThen {
                    Phase::PreserveOuterError
                } else {
                    Phase::Wrap(1)
                }
            }
            StandardIntrinsic::ResultMapErr => {
                if success {
                    Phase::Wrap(0)
                } else {
                    invoke(1, true, AfterCallback::PreserveError)
                }
            }
            StandardIntrinsic::OptionOkOr => {
                if !success {
                    self.result(runtime, roots, roots.get(1).ok_or_else(invalid)?)?;
                }
                Phase::Wrap(self.variant)
            }
            StandardIntrinsic::OptionOkOrElse => {
                if success {
                    Phase::Wrap(0)
                } else {
                    invoke(1, false, AfterCallback::Wrap(1))
                }
            }
            StandardIntrinsic::OptionOrElse | StandardIntrinsic::ResultOrElse => {
                if success {
                    Phase::Wrap(0)
                } else {
                    invoke(
                        1,
                        self.operation == StandardIntrinsic::ResultOrElse,
                        AfterCallback::Move,
                    )
                }
            }
            StandardIntrinsic::OptionMapOr | StandardIntrinsic::ResultMapOr => {
                if success {
                    invoke(2, true, AfterCallback::Move)
                } else {
                    self.result(runtime, roots, roots.get(1).ok_or_else(invalid)?)?;
                    Phase::Move
                }
            }
            StandardIntrinsic::OptionMapOrElse | StandardIntrinsic::ResultMapOrElse => {
                if success {
                    invoke(2, true, AfterCallback::Move)
                } else {
                    invoke(
                        1,
                        self.operation == StandardIntrinsic::ResultMapOrElse,
                        AfterCallback::Move,
                    )
                }
            }
            StandardIntrinsic::OptionFilter => {
                if success {
                    invoke(1, true, AfterCallback::Filter)
                } else {
                    self.result(runtime, roots, roots.get(0).ok_or_else(invalid)?)?;
                    Phase::Move
                }
            }
            StandardIntrinsic::OptionIsSomeAnd
            | StandardIntrinsic::ResultIsOkAnd
            | StandardIntrinsic::ResultIsErrAnd => {
                if (self.variant == 1) == (self.operation == StandardIntrinsic::ResultIsErrAnd) {
                    invoke(1, true, AfterCallback::Move)
                } else {
                    Phase::ConstantFalse
                }
            }
            StandardIntrinsic::OptionZip => {
                if success {
                    Phase::InnerTest
                } else {
                    Phase::Wrap(1)
                }
            }
            StandardIntrinsic::OptionFlatten | StandardIntrinsic::ResultFlatten => {
                if success {
                    Phase::Move
                } else if self.operation == StandardIntrinsic::ResultFlatten {
                    Phase::PreserveOuterError
                } else {
                    Phase::Wrap(1)
                }
            }
            StandardIntrinsic::ResultOk | StandardIntrinsic::ResultErr => Phase::Wrap(
                if (self.variant == 1) == (self.operation == StandardIntrinsic::ResultErr) {
                    0
                } else {
                    1
                },
            ),
            StandardIntrinsic::OptionTranspose => {
                if success {
                    Phase::InnerTest
                } else {
                    Phase::NestedEmpty
                }
            }
            StandardIntrinsic::ResultTranspose => {
                if success {
                    Phase::InnerTest
                } else {
                    Phase::NestedError
                }
            }
            _ => {
                return Err(RuntimeError::module_validation(
                    "invalid native enum binding",
                ));
            }
        };
        Ok(phase)
    }

    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        implementation: &LoadedModule,
        signature: &NativeSignature,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::Branch => {
                self.phase = if self.variant == 0
                    || family(&signature.params[0])?.0 == StandardEnum::Result
                {
                    Phase::ReadOuter
                } else {
                    self.select(runtime, roots)?
                };
            }
            Phase::ReadOuter => {
                let value = read(
                    runtime,
                    &roots.get(0).ok_or_else(invalid)?,
                    &signature.params[0],
                    self.variant,
                )?;
                self.set(runtime, roots, PAYLOAD, value.clone())?;
                self.result(runtime, roots, value)?;
                self.phase = self.select(runtime, roots)?;
            }
            Phase::Invoke {
                slot,
                payload,
                after,
            } => {
                let request = callback(
                    runtime,
                    &roots.get(slot).ok_or_else(invalid)?,
                    &signature.params[slot],
                    if payload {
                        vec![self.get(roots, PAYLOAD)?]
                    } else {
                        vec![]
                    },
                )?;
                self.phase = Phase::Waiting(after);
                return Ok(NativeAction::Callback(request));
            }
            Phase::Waiting(_) => {
                return Err(RuntimeError::module_validation(
                    "native enum callback has not returned",
                ));
            }
            Phase::Wrap(selected) => {
                self.result(
                    runtime,
                    roots,
                    make(
                        runtime,
                        &signature.result,
                        selected,
                        self.get(roots, RESULT)?,
                    )?,
                )?;
                self.phase = self.move_phase();
            }
            Phase::PreserveOuterError => {
                self.result(
                    runtime,
                    roots,
                    runtime.map_result_error(
                        implementation,
                        &roots.get(0).ok_or_else(invalid)?,
                        self.get(roots, RESULT)?,
                        &signature.result,
                    )?,
                )?;
                self.phase = Phase::Move;
            }
            Phase::ConstantFalse => {
                self.result(runtime, roots, Value::Bool(false))?;
                self.phase = Phase::Move;
            }
            Phase::InnerTest => {
                let (value, ty) = if self.operation == StandardIntrinsic::OptionZip {
                    (roots.get(1).ok_or_else(invalid)?, &signature.params[1])
                } else {
                    (
                        self.get(roots, PAYLOAD)?,
                        &family(&signature.params[0])?.1[0],
                    )
                };
                self.phase = Phase::InnerBranch(variant(runtime, &value, ty)?);
            }
            Phase::InnerBranch(selected) => {
                self.inner = true;
                self.phase =
                    if selected == 0 || self.operation == StandardIntrinsic::OptionTranspose {
                        Phase::InnerRead(selected)
                    } else {
                        Phase::Wrap(1)
                    };
            }
            Phase::FilterBranch => {
                self.inner = true;
                let Value::Bool(keep) = self.get(roots, RESULT)? else {
                    return Err(invalid());
                };
                self.phase = if keep {
                    self.result(runtime, roots, roots.get(0).ok_or_else(invalid)?)?;
                    Phase::InnerMove
                } else {
                    Phase::Wrap(1)
                };
            }
            Phase::InnerRead(selected) => {
                let (value, ty) = if self.operation == StandardIntrinsic::OptionZip {
                    (roots.get(1).ok_or_else(invalid)?, &signature.params[1])
                } else {
                    (
                        self.get(roots, PAYLOAD)?,
                        &family(&signature.params[0])?.1[0],
                    )
                };
                let value = read(runtime, &value, ty, selected)?;
                self.phase = if self.operation == StandardIntrinsic::OptionZip {
                    self.set(runtime, roots, SECOND, value)?;
                    Phase::ZipTuple
                } else {
                    self.result(runtime, roots, value)?;
                    if selected == 1 {
                        Phase::PreserveInnerError
                    } else {
                        Phase::NestedWrap
                    }
                };
            }
            Phase::ZipTuple => {
                self.result(
                    runtime,
                    roots,
                    Value::Tuple(vec![self.get(roots, PAYLOAD)?, self.get(roots, SECOND)?]),
                )?;
                self.phase = Phase::Wrap(0);
            }
            Phase::NestedWrap | Phase::NestedEmpty => {
                let selected = u32::from(matches!(self.phase, Phase::NestedEmpty));
                self.result(
                    runtime,
                    roots,
                    make(
                        runtime,
                        &family(&signature.result)?.1[0],
                        selected,
                        self.get(roots, RESULT)?,
                    )?,
                )?;
                self.phase = Phase::Wrap(0);
            }
            Phase::NestedError => {
                self.result(
                    runtime,
                    roots,
                    runtime.map_result_error(
                        implementation,
                        &roots.get(0).ok_or_else(invalid)?,
                        self.get(roots, RESULT)?,
                        &family(&signature.result)?.1[0],
                    )?,
                )?;
                self.phase = Phase::Wrap(0);
            }
            Phase::PreserveInnerError => {
                self.result(
                    runtime,
                    roots,
                    runtime.map_result_error(
                        implementation,
                        &self.get(roots, PAYLOAD)?,
                        self.get(roots, RESULT)?,
                        &signature.result,
                    )?,
                )?;
                self.phase = Phase::InnerMove;
            }
            // The inner join has its own Move and Jump before the outer join.
            Phase::InnerMove => self.phase = Phase::InnerJump,
            Phase::InnerJump => self.phase = Phase::Move,
            Phase::Move => {
                let value = self.get(roots, RESULT)?;
                if !runtime.matches_interface_method_abi(&value, &signature.result, implementation)
                {
                    return Err(invalid());
                }
                self.phase = Phase::Jump;
                return Ok(NativeAction::Publish(value));
            }
            Phase::Jump => return Ok(NativeAction::Finish),
        }
        Ok(NativeAction::Continue)
    }

    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let Phase::Waiting(after) = self.phase else {
            return Err(RuntimeError::module_validation(
                "unexpected native enum callback result",
            ));
        };
        self.result(runtime, roots, value)?;
        self.phase = match after {
            AfterCallback::Move => Phase::Move,
            AfterCallback::Wrap(selected) => Phase::Wrap(selected),
            AfterCallback::PreserveError => Phase::PreserveOuterError,
            AfterCallback::Filter => Phase::FilterBranch,
        };
        Ok(())
    }
}

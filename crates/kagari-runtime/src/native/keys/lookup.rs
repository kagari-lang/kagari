//! Hash once, compare stable bucket candidates, then release the guard and commit.
use super::{Buffers, CANDIDATE, CANDIDATES, KEY, invalid, witness};
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{CollectionIteration, RootSet},
    native::{
        NativeAction,
        protocols::{self, ProtocolStep},
    },
    value::Value,
};
use kagari_abi::{
    native_import::EngineNativeImport,
    scalar::BuiltinType,
    standard::{StandardIntrinsic, traits::StandardTrait},
    types::AbiType,
};
enum Phase {
    Begin,
    Hash,
    WaitingHash,
    Candidates,
    Length,
    Token,
    Index,
    Jump,
    More,
    Branch,
    Read,
    Zero,
    One,
    ReadToken,
    ReadKey,
    Equal,
    WaitingEqual,
    EqualBranch,
    Found,
    FoundJump,
    IncrementOne,
    Increment,
    MoveIndex,
    Commit,
    Contains,
}
pub(super) enum KeyStep {
    Action(NativeAction),
    Ready(Value),
}
pub(super) struct Lookup {
    operation: StandardIntrinsic,
    custom: bool,
    buffers: Buffers,
    payload: Option<usize>,
    phase: Phase,
    guard: Option<CollectionIteration>,
    hash: i64,
    token: i64,
    candidate: i64,
    length: u64,
    index: u64,
    next: u64,
    equal: bool,
}
impl Lookup {
    pub(super) fn start(
        operation: StandardIntrinsic,
        custom: bool,
        buffers: Buffers,
        payload: Option<usize>,
    ) -> Self {
        Self {
            operation,
            custom,
            buffers,
            payload,
            phase: Phase::Begin,
            guard: None,
            hash: 0,
            token: -1,
            candidate: -1,
            length: 0,
            index: 0,
            next: 0,
            equal: false,
        }
    }
    fn values(&self, roots: &RootSet) -> Result<Vec<Value>, RuntimeError> {
        let mut args = vec![self.buffers.receiver(roots)?, self.buffers.query(roots)?];
        if self.operation == StandardIntrinsic::MapInsert {
            args.push(
                roots
                    .get(self.payload.ok_or_else(invalid)?)
                    .ok_or_else(invalid)?,
            );
        }
        Ok(args)
    }
    pub(super) fn begin(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<KeyStep, RuntimeError> {
        if !matches!(self.phase, Phase::Begin) {
            return Err(invalid());
        }
        if !self.custom {
            return Ok(
                match runtime.invoke_standard_builtin(self.operation, &self.values(roots)?) {
                    Ok(value) => KeyStep::Ready(value),
                    Err(error) => KeyStep::Action(NativeAction::BuiltinFailure(error)),
                },
            );
        }
        self.guard = Some(
            runtime
                .gc()
                .begin_key_lookup(&self.buffers.receiver(roots)?)?,
        );
        self.phase = Phase::Hash;
        Ok(KeyStep::Action(NativeAction::Continue))
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        let expected = if matches!(self.phase, Phase::WaitingHash) {
            BuiltinType::I64
        } else if matches!(self.phase, Phase::WaitingEqual) {
            BuiltinType::Bool
        } else {
            return Err(invalid());
        };
        if !runtime.matches_interface_method_abi(&value, &AbiType::Builtin(expected), owner) {
            return Err(invalid());
        }
        match value {
            Value::I64(hash) if expected == BuiltinType::I64 => {
                self.hash = hash;
                self.phase = Phase::Candidates;
            }
            Value::Bool(equal) if expected == BuiltinType::Bool => {
                self.equal = equal;
                self.phase = Phase::EqualBranch;
            }
            _ => return Err(invalid()),
        };
        Ok(NativeAction::Continue)
    }
    fn protocol(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        step: ProtocolStep,
        hash: bool,
    ) -> Result<KeyStep, RuntimeError> {
        self.phase = if hash {
            Phase::WaitingHash
        } else {
            Phase::WaitingEqual
        };
        Ok(KeyStep::Action(match step {
            ProtocolStep::Value(value) => self.receive(runtime, owner, value)?,
            ProtocolStep::Call(request) => NativeAction::Callback(request),
            ProtocolStep::BuiltinFailure(error) => NativeAction::BuiltinFailure(error),
        }))
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<KeyStep, RuntimeError> {
        match self.phase {
            Phase::Begin => return self.begin(runtime, roots),
            Phase::Hash => {
                let step = protocols::hash(
                    runtime,
                    owner,
                    witness(contract, StandardTrait::Hash)?,
                    self.buffers.query(roots)?,
                )?;
                return self.protocol(runtime, owner, step, true);
            }
            Phase::Candidates => {
                match runtime.invoke_standard_builtin(
                    StandardIntrinsic::KeyCandidates,
                    &[self.buffers.receiver(roots)?, Value::I64(self.hash)],
                ) {
                    Ok(value) => self.buffers.set(runtime, roots, CANDIDATES, value)?,
                    Err(error) => return Ok(KeyStep::Action(NativeAction::BuiltinFailure(error))),
                }
                self.phase = Phase::Length;
            }
            Phase::Length => {
                let Value::Array(id) = self.buffers.get(roots, CANDIDATES)? else {
                    return Err(invalid());
                };
                self.length = runtime.gc().array_len(id).ok_or_else(invalid)? as u64;
                self.phase = Phase::Token;
            }
            Phase::Token => {
                self.token = -1;
                self.phase = Phase::Index;
            }
            Phase::Index => {
                self.index = 0;
                self.phase = Phase::Jump;
            }
            Phase::Jump => self.phase = Phase::More,
            Phase::More => {
                self.equal = self.index < self.length;
                self.phase = Phase::Branch;
            }
            Phase::Branch => {
                self.phase = if self.equal {
                    Phase::Read
                } else {
                    Phase::Commit
                }
            }
            Phase::Read => {
                let Value::Array(id) = self.buffers.get(roots, CANDIDATES)? else {
                    return Err(invalid());
                };
                let value = runtime
                    .gc()
                    .array_get(id, usize::try_from(self.index).map_err(|_| invalid())?)
                    .ok_or_else(invalid)?;
                self.buffers.set(runtime, roots, CANDIDATE, value)?;
                self.phase = Phase::Zero;
            }
            Phase::Zero => self.phase = Phase::One,
            Phase::One => self.phase = Phase::ReadToken,
            Phase::ReadToken => {
                let Value::Tuple(values) = self.buffers.get(roots, CANDIDATE)? else {
                    return Err(invalid());
                };
                let Some(Value::I64(token)) = values.first() else {
                    return Err(invalid());
                };
                self.candidate = *token;
                self.phase = Phase::ReadKey;
            }
            Phase::ReadKey => {
                let Value::Tuple(values) = self.buffers.get(roots, CANDIDATE)? else {
                    return Err(invalid());
                };
                self.buffers.set(
                    runtime,
                    roots,
                    KEY,
                    values.get(1).cloned().ok_or_else(invalid)?,
                )?;
                self.phase = Phase::Equal;
            }
            Phase::Equal => {
                let step = protocols::equal(
                    runtime,
                    owner,
                    witness(contract, StandardTrait::PartialEq)?,
                    self.buffers.query(roots)?,
                    self.buffers.get(roots, KEY)?,
                )?;
                return self.protocol(runtime, owner, step, false);
            }
            Phase::EqualBranch => {
                self.phase = if self.equal {
                    Phase::Found
                } else {
                    Phase::IncrementOne
                }
            }
            Phase::Found => {
                self.token = self.candidate;
                self.phase = Phase::FoundJump;
            }
            Phase::FoundJump => self.phase = Phase::Commit,
            Phase::IncrementOne => self.phase = Phase::Increment,
            Phase::Increment => {
                self.next = self.index.checked_add(1).ok_or_else(invalid)?;
                self.phase = Phase::MoveIndex;
            }
            Phase::MoveIndex => {
                self.index = self.next;
                self.phase = Phase::Jump;
            }
            Phase::Commit => {
                self.guard.take();
                let operation = match self.operation {
                    StandardIntrinsic::MapGet | StandardIntrinsic::MapContainsKey => {
                        StandardIntrinsic::KeyMapGet
                    }
                    StandardIntrinsic::MapInsert => StandardIntrinsic::KeyMapInsert,
                    StandardIntrinsic::MapRemove => StandardIntrinsic::KeyMapRemove,
                    StandardIntrinsic::SetContains => StandardIntrinsic::KeySetContains,
                    StandardIntrinsic::SetInsert => StandardIntrinsic::KeySetInsert,
                    StandardIntrinsic::SetRemove => StandardIntrinsic::KeySetRemove,
                    _ => return Err(invalid()),
                };
                let mut args = vec![
                    self.buffers.receiver(roots)?,
                    Value::I64(self.hash),
                    Value::I64(self.token),
                ];
                if matches!(
                    self.operation,
                    StandardIntrinsic::MapInsert | StandardIntrinsic::SetInsert
                ) {
                    args.push(self.buffers.query(roots)?);
                }
                if self.operation == StandardIntrinsic::MapInsert {
                    args.push(
                        roots
                            .get(self.payload.ok_or_else(invalid)?)
                            .ok_or_else(invalid)?,
                    );
                }
                return Ok(match runtime.invoke_standard_builtin(operation, &args) {
                    Ok(value) if self.operation == StandardIntrinsic::MapContainsKey => {
                        self.buffers.set(runtime, roots, super::RESULT, value)?;
                        self.phase = Phase::Contains;
                        KeyStep::Action(NativeAction::Continue)
                    }
                    Ok(value) => KeyStep::Ready(value),
                    Err(error) => KeyStep::Action(NativeAction::BuiltinFailure(error)),
                });
            }
            Phase::Contains => {
                return Ok(
                    match runtime.invoke_standard_builtin(
                        StandardIntrinsic::OptionIsSome,
                        &[self.buffers.get(roots, super::RESULT)?],
                    ) {
                        Ok(value) => KeyStep::Ready(value),
                        Err(error) => KeyStep::Action(NativeAction::BuiltinFailure(error)),
                    },
                );
            }
            Phase::WaitingHash | Phase::WaitingEqual => return Err(invalid()),
        }
        Ok(KeyStep::Action(NativeAction::Continue))
    }
}

//! Rooted predicate traversal followed by one token-preserving storage commit.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{CollectionIteration, RootSet},
    native::{NativeAction, callback},
    value::{EnumTag, Value},
};
use kagari_abi::{
    native_import::NativeSignature,
    operations::IterOp,
    scalar::BuiltinType,
    standard::{StandardIntrinsic, surface::StandardEnum},
    types::AbiType,
};
const MASK: usize = 0;
const ITERATOR: usize = 1;
const NEXT: usize = 2;
const ITEM: usize = 3;
const KEY: usize = 4;
const VALUE: usize = 5;
const KEEP: usize = 6;
pub(super) const SCRATCH_ROOTS: usize = 7;
#[derive(Clone, Copy)]
enum Phase {
    Mask,
    Iterator,
    Begin,
    Jump,
    Next,
    Move,
    Test,
    Branch,
    Read,
    KeyIndex,
    Key,
    ValueIndex,
    Value,
    Invoke,
    Waiting,
    Append,
    Close,
    EndIteration,
    EndMutation,
    Commit,
}
pub(super) struct Retention {
    scratch: usize,
    phase: Phase,
    iterator: AbiType,
    optional: AbiType,
    map: bool,
    present: bool,
    mutation: Option<CollectionIteration>,
    iteration: Option<CollectionIteration>,
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native retention contract mismatch")
}
impl Retention {
    pub(super) fn start(
        signature: &NativeSignature,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let item = match signature.params.first() {
            Some(AbiType::Array(item, _) | AbiType::Set(item, _)) => item.as_ref().clone(),
            Some(AbiType::Map { key, value, .. }) => {
                AbiType::Tuple(vec![key.as_ref().clone(), value.as_ref().clone()])
            }
            _ => return Err(invalid()),
        };
        Ok(Self {
            scratch: arguments.len(),
            phase: Phase::Mask,
            iterator: AbiType::Iter(Box::new(item.clone())),
            optional: AbiType::StandardEnum {
                kind: StandardEnum::Option,
                args: vec![item],
            },
            map: matches!(signature.params.first(), Some(AbiType::Map { .. })),
            present: false,
            mutation: None,
            iteration: None,
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
    /// The already charged mutation guard is acquired after argument roots exist.
    pub(super) fn initialize(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<(), RuntimeError> {
        self.mutation = Some(
            runtime
                .gc()
                .begin_collection_mutation(&roots.get(0).ok_or_else(invalid)?)?,
        );
        Ok(())
    }
    fn field(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
        index: usize,
        slot: usize,
    ) -> Result<(), RuntimeError> {
        let Value::Tuple(fields) = self.get(roots, ITEM)? else {
            return Err(invalid());
        };
        self.set(
            runtime,
            roots,
            slot,
            fields.get(index).cloned().ok_or_else(invalid)?,
        )
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        roots: &RootSet,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        if !matches!(self.phase, Phase::Waiting)
            || !runtime.matches_interface_method_abi(
                &value,
                &AbiType::Builtin(BuiltinType::Bool),
                owner,
            )
        {
            return Err(invalid());
        }
        self.set(runtime, roots, KEEP, value)?;
        self.phase = Phase::Append;
        Ok(NativeAction::Continue)
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        signature: &NativeSignature,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::Mask => {
                let value =
                    match runtime.invoke_standard_builtin(StandardIntrinsic::ArrayListNew, &[]) {
                        Ok(value) => value,
                        Err(error) => return Ok(NativeAction::BuiltinFailure(error)),
                    };
                self.set(runtime, roots, MASK, value)?;
                self.phase = Phase::Iterator;
            }
            Phase::Iterator => {
                let value = runtime.iter_operation(
                    owner,
                    &roots.get(0).ok_or_else(invalid)?,
                    &signature.params[0],
                    IterOp::New,
                )?;
                self.set(runtime, roots, ITERATOR, value)?;
                self.phase = Phase::Begin;
            }
            Phase::Begin => {
                self.iteration = Some(
                    runtime
                        .gc()
                        .begin_collection_iteration(&self.get(roots, ITERATOR)?)?,
                );
                self.phase = Phase::Jump;
            }
            Phase::Jump => self.phase = Phase::Next,
            Phase::Next => {
                let value = runtime.iter_operation(
                    owner,
                    &self.get(roots, ITERATOR)?,
                    &self.iterator,
                    IterOp::Next,
                )?;
                if !runtime.matches_interface_method_abi(&value, &self.optional, owner) {
                    return Err(invalid());
                }
                self.set(runtime, roots, NEXT, value)?;
                self.phase = Phase::Move;
            }
            Phase::Move => self.phase = Phase::Test,
            Phase::Test => {
                let Value::Enum(id) = self.get(roots, NEXT)? else {
                    return Err(invalid());
                };
                self.present =
                    runtime.gc().enum_snapshot(id).ok_or_else(invalid)?.tag == EnumTag::OptionSome;
                self.phase = Phase::Branch;
            }
            Phase::Branch => {
                self.phase = if self.present {
                    Phase::Read
                } else {
                    Phase::Close
                }
            }
            Phase::Read => {
                let Value::Enum(id) = self.get(roots, NEXT)? else {
                    return Err(invalid());
                };
                let value = runtime
                    .gc()
                    .enum_snapshot(id)
                    .ok_or_else(invalid)?
                    .fields
                    .into_iter()
                    .next()
                    .ok_or_else(invalid)?;
                self.set(runtime, roots, ITEM, value)?;
                self.phase = if self.map {
                    Phase::KeyIndex
                } else {
                    Phase::Invoke
                };
            }
            Phase::KeyIndex => self.phase = Phase::Key,
            Phase::Key => {
                self.field(runtime, roots, 0, KEY)?;
                self.phase = Phase::ValueIndex;
            }
            Phase::ValueIndex => self.phase = Phase::Value,
            Phase::Value => {
                self.field(runtime, roots, 1, VALUE)?;
                self.phase = Phase::Invoke;
            }
            Phase::Invoke => {
                let arguments = if self.map {
                    vec![self.get(roots, KEY)?, self.get(roots, VALUE)?]
                } else {
                    vec![self.get(roots, ITEM)?]
                };
                let request = callback(
                    runtime,
                    &roots.get(1).ok_or_else(invalid)?,
                    &signature.params[1],
                    arguments,
                )?;
                self.phase = Phase::Waiting;
                return Ok(NativeAction::Callback(request));
            }
            Phase::Append => {
                if let Err(error) = runtime.invoke_standard_builtin(
                    StandardIntrinsic::ArrayPush,
                    &[self.get(roots, MASK)?, self.get(roots, KEEP)?],
                ) {
                    return Ok(NativeAction::BuiltinFailure(error));
                }
                self.phase = Phase::Jump;
            }
            Phase::Close => {
                runtime.iter_operation(
                    owner,
                    &self.get(roots, ITERATOR)?,
                    &self.iterator,
                    IterOp::Close,
                )?;
                self.phase = Phase::EndIteration;
            }
            Phase::EndIteration => {
                self.iteration.take();
                self.phase = Phase::EndMutation;
            }
            Phase::EndMutation => {
                self.mutation.take();
                self.phase = Phase::Commit;
            }
            Phase::Commit => {
                return Ok(
                    match runtime.invoke_standard_builtin(
                        StandardIntrinsic::CollectionRetainStorage,
                        &[roots.get(0).ok_or_else(invalid)?, self.get(roots, MASK)?],
                    ) {
                        Ok(value) => NativeAction::Complete(value),
                        Err(error) => NativeAction::BuiltinFailure(error),
                    },
                );
            }
            Phase::Waiting => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}

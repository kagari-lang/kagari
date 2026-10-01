//! Owned preparation buffers protect a target until one checked storage commit.
use crate::{
    gc::{CollectionIteration, HeapObjectId, RootSet, mutations::PreparedCollectionCommit},
    native::NativeContext,
    native_value::{NativeCall, NativeResult, NativeValue, invalid},
    value::Value,
};
use kagari_abi::{operations::IterOp, types::AbiType};
use kagari_common::collection::CollectionAccess;
use std::marker::PhantomData;

/// Buffers are ordinary GC storage; dropping preparation releases roots and guards.
/// Completed callback/payload effects survive an unsuccessful preparation.
pub struct NativeReorder<T: NativeValue> {
    call: NativeCall,
    target: HeapObjectId,
    item: AbiType,
    roots: RootSet,
    mutation: Option<CollectionIteration>,
    iteration: Option<CollectionIteration>,
    _type: PhantomData<T>,
}
const TARGET: usize = 0;
const INPUT: usize = 1;
const OUTPUT: usize = 2;
const ITERATOR: usize = 3;

impl<T: NativeValue> NativeReorder<T> {
    pub(super) fn new(call: NativeCall, target: HeapObjectId, item: AbiType) -> NativeResult<Self> {
        let value = Value::Array(target);
        let mutation = Some(call.heap.begin_collection_mutation(&value)?);
        let roots = call
            .heap
            .root_execution_values(vec![value, Value::Unit, Value::Unit, Value::Unit])
            .ok_or_else(invalid)?;
        Ok(Self {
            call,
            target,
            item,
            roots,
            mutation,
            iteration: None,
            _type: PhantomData,
        })
    }
    pub fn len(&self) -> usize {
        self.call
            .heap
            .array_len(self.target)
            .expect("rooted preparation target")
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn buffer(&self, slot: usize) -> NativeResult<HeapObjectId> {
        match self.roots.get(slot) {
            Some(Value::Array(id)) => Ok(id),
            _ => Err(invalid()),
        }
    }
    fn allocate(&self, slot: usize) -> NativeResult<()> {
        let value = Value::Array(self.call.heap.alloc_array(vec![])?);
        self.roots
            .set(&self.call.heap, slot, value)
            .ok_or_else(invalid)
    }
    pub fn allocate_input(&self) -> NativeResult<()> {
        self.allocate(INPUT)
    }
    pub fn allocate_output(&self) -> NativeResult<()> {
        self.allocate(OUTPUT)
    }
    fn read(&self, id: HeapObjectId, index: usize) -> NativeResult<T> {
        let value = self.call.heap.array_get(id, index).ok_or_else(invalid)?;
        let scope = self.call.conversion_scope();
        scope.check(&value, &self.item)?;
        T::read(&scope, value, &self.item)
    }
    pub fn read_original(&self, index: usize) -> NativeResult<T> {
        self.read(self.target, index)
    }
    pub fn read_input(&self, index: usize) -> NativeResult<T> {
        self.read(self.buffer(INPUT)?, index)
    }
    fn push(&self, slot: usize, value: T) -> NativeResult<()> {
        let scope = self.call.conversion_scope();
        self.call
            .heap
            .array_push(self.buffer(slot)?, value.write(&scope, &self.item)?)
    }
    pub fn push_input(&self, value: T) -> NativeResult<()> {
        self.push(INPUT, value)
    }
    pub fn push_output(&self, value: T) -> NativeResult<()> {
        self.push(OUTPUT, value)
    }
    pub fn publish_output(&self) -> NativeResult<()> {
        self.roots
            .set(
                &self.call.heap,
                INPUT,
                self.roots.get(OUTPUT).ok_or_else(invalid)?,
            )
            .ok_or_else(invalid)
    }
    pub(crate) fn create_iterator(&self, context: &NativeContext<'_>) -> NativeResult<()> {
        let value = context.iterator_operation(
            &self.call.owner,
            &Value::Array(self.target),
            &AbiType::Array(Box::new(self.item.clone()), CollectionAccess::Mutable),
            IterOp::New,
        )?;
        self.roots
            .set(&self.call.heap, ITERATOR, value)
            .ok_or_else(invalid)
    }
    pub(crate) fn begin_iteration(&mut self) -> NativeResult<()> {
        self.iteration = Some(
            self.call
                .heap
                .begin_collection_iteration(&self.roots.get(ITERATOR).ok_or_else(invalid)?)?,
        );
        Ok(())
    }
    pub(crate) fn close_iterator(&self, context: &NativeContext<'_>) -> NativeResult<()> {
        context.iterator_operation(
            &self.call.owner,
            &self.roots.get(ITERATOR).ok_or_else(invalid)?,
            &AbiType::Iter(Box::new(self.item.clone())),
            IterOp::Close,
        )?;
        Ok(())
    }
    pub(crate) fn end_iteration(&mut self) {
        self.iteration.take();
    }
    pub(crate) fn end_mutation(&mut self) {
        self.mutation.take();
    }
    /// Validates structural guards, budgets and allocation before changing slots.
    pub fn commit(mut self) -> NativeResult<()> {
        self.mutation.take();
        self.call.heap.commit_prepared_collection(
            PreparedCollectionCommit::ReplaceArray,
            &[
                self.roots.get(TARGET).ok_or_else(invalid)?,
                self.roots.get(INPUT).ok_or_else(invalid)?,
            ],
        )
    }
}

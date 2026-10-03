//! Comparators and key selectors run synchronously, with no key-cache prepass.
use crate::{
    native::{
        binding::NativeResult,
        callable::CallableHandle,
        context::{CallContext, LinkedCallable},
        declarations::SelectedCall,
        foundation::lists::{Algorithm, invalid},
        scalar::NativeScalar,
        sequence_edit::SequenceEdit,
    },
    value::{EnumTag, Value},
    value_semantics,
};
use kagari_abi::{scalar::BuiltinType, standard::RuntimePrimitive, types::AbiType};
use std::cmp::Ordering;

pub(super) struct Comparison<'call> {
    operation: Option<&'call LinkedCallable>,
    callback: Option<CallableHandle<'call>>,
    algorithm: Algorithm,
}

impl<'call> Comparison<'call> {
    pub(super) fn prepare(cx: &CallContext<'call>, algorithm: Algorithm) -> NativeResult<Self> {
        Ok(Self {
            operation: algorithm
                .comparison()
                .then(|| cx.selected(SelectedCall { slot: 0 }))
                .transpose()?,
            callback: matches!(
                algorithm,
                Algorithm::SortBy | Algorithm::SortByKey | Algorithm::Retain
            )
            .then(|| cx.callable(1))
            .transpose()?,
            algorithm,
        })
    }

    pub(super) fn keep(&self, cx: &mut CallContext<'_>, value: Value) -> NativeResult<bool> {
        bool::decode(
            self.callback
                .as_ref()
                .ok_or_else(invalid)?
                .call_values(cx, &[value])?,
        )
    }

    pub(super) fn equal(&self, cx: &mut CallContext<'_>, a: Value, b: Value) -> NativeResult<bool> {
        cx.poll()?;
        let operation = self.operation.ok_or_else(invalid)?;
        if operation.primitive == Some(RuntimePrimitive::ValueEq) {
            return value_semantics::script_equal(cx.heap(), &a, &b);
        }
        bool::decode(cx.call_values(operation, &[a, b])?)
    }

    pub(super) fn order(
        &self,
        cx: &mut CallContext<'_>,
        a: Value,
        b: Value,
    ) -> NativeResult<Ordering> {
        cx.poll()?;
        if matches!(self.algorithm, Algorithm::SortBy) {
            let result = self
                .callback
                .as_ref()
                .ok_or_else(invalid)?
                .call_values(cx, &[a, b])?;
            return decode_ordering(cx, result);
        }
        let (a, b, _root) = if matches!(self.algorithm, Algorithm::SortByKey) {
            let selector = self.callback.as_ref().ok_or_else(invalid)?;
            let a = selector.call_values(cx, &[a])?;
            let root = cx.heap().root_value(a.clone()).ok_or_else(invalid)?;
            let b = selector.call_values(cx, &[b])?;
            (a, b, Some(root))
        } else {
            (a, b, None)
        };
        let operation = self.operation.ok_or_else(invalid)?;
        if operation.primitive == Some(RuntimePrimitive::ValueCmp) {
            return value_semantics::builtin_order(cx.heap(), &a, &b)?.ok_or_else(invalid);
        }
        let result = cx.call_values(operation, &[a, b])?;
        decode_ordering(cx, result)
    }

    pub(super) fn sort_scalars(&self, values: &mut SequenceEdit<'_>) -> NativeResult<bool> {
        let operation = self.operation.ok_or_else(invalid)?;
        if operation.primitive != Some(RuntimePrimitive::ValueCmp) {
            return Ok(false);
        }
        macro_rules! scalars {
            ($($kind:ident:$ty:ty),+) => { match operation.params[0] {
                $(AbiType::Builtin(BuiltinType::$kind) => values.with_slice_mut::<$ty, _>(|items| { items.sort(); Ok(()) })?,)+
                _ => return Ok(false),
            } };
        }

        scalars!(Unit:(), Bool:bool, I8:i8, I16:i16, I32:i32, I64:i64, ISize:isize, U8:u8, U16:u16, U32:u32, U64:u64, USize:usize);
        Ok(true)
    }
}

fn decode_ordering(cx: &CallContext<'_>, value: Value) -> NativeResult<Ordering> {
    let Value::Enum(id) = value else {
        return Err(invalid());
    };
    match cx.heap().enum_snapshot(id).ok_or_else(invalid)?.tag {
        EnumTag::OrderingLess => Ok(Ordering::Less),
        EnumTag::OrderingEqual => Ok(Ordering::Equal),
        EnumTag::OrderingGreater => Ok(Ordering::Greater),
        _ => Err(invalid()),
    }
}

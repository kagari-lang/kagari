//! Exclusive direct edits. A lease roots traced elements across synchronous callbacks
//! and restores the edited storage even when a callback fails or unwinds.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::NativeResult,
        scalar::NativeScalar,
        sequence::{NativeElement, SequenceStorage},
    },
    value::Value,
};
use std::{
    cmp::Ordering,
    panic::{AssertUnwindSafe, catch_unwind},
};

pub struct SequenceEdit<'buffer> {
    pub(crate) values: &'buffer mut SequenceStorage,
}

impl SequenceEdit<'_> {
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> NativeResult<Value> {
        self.values
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("sequence edit index"))
    }

    pub fn with_slice_mut<E: NativeElement, R>(
        &mut self,
        access: impl for<'slice> FnOnce(&'slice mut [E]) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let values = E::slice_mut(self.values)
            .ok_or_else(|| RuntimeError::module_validation("sequence edit scalar layout"))?;
        access(values)
    }

    pub fn reverse(&mut self) {
        self.values.reverse();
    }
}

macro_rules! edits {
    ($($variant:ident),+) => {
        impl SequenceEdit<'_> {
            /// Stable Rust sorting preserves elements on failure. Stop invoking
            /// user code after the first error while Rust finishes its bookkeeping.
            pub fn sort_by(&mut self, mut compare: impl FnMut(Value, Value) -> NativeResult<Ordering>) -> NativeResult<()> {
                match self.values {
                    $(SequenceStorage::$variant(values) => sort(values, |a, b| compare(a.encode(), b.encode())),)+
                    SequenceStorage::Traced(values) => sort(values, compare),
                }
            }

            /// Successful removals remain visible when a later predicate fails.
            pub fn retain(&mut self, mut keep: impl FnMut(Value) -> NativeResult<bool>) -> NativeResult<()> {
                match self.values {
                    $(SequenceStorage::$variant(values) => retain(values, |value| keep(value.encode())),)+
                    SequenceStorage::Traced(values) => retain(values, keep),
                }
            }

            pub fn dedup_by(&mut self, mut equal: impl FnMut(Value, Value) -> NativeResult<bool>) -> NativeResult<()> {
                match self.values {
                    $(SequenceStorage::$variant(values) => dedup(values, |a, b| equal(a.encode(), b.encode())),)+
                    SequenceStorage::Traced(values) => dedup(values, equal),
                }
            }
        }
    };
}

edits!(
    Unit, Bool, I8, I16, I32, I64, ISize, U8, U16, U32, U64, USize, F32, F64
);

fn sort<T: Clone>(
    values: &mut [T],
    mut compare: impl FnMut(T, T) -> NativeResult<Ordering>,
) -> NativeResult<()> {
    let mut failure = None;
    let result = catch_unwind(AssertUnwindSafe(|| {
        values.sort_by(|left, right| {
            if failure.is_some() {
                return Ordering::Equal;
            }
            match compare(left.clone(), right.clone()) {
                Ok(order) => order,
                Err(error) => {
                    failure = Some(error);
                    Ordering::Equal
                }
            }
        })
    }));
    if let Some(error) = failure {
        return Err(error);
    }
    result.map_err(|_| {
        RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "sort comparator does not define a consistent order",
        )
    })
}

fn retain<T: Clone>(
    values: &mut Vec<T>,
    mut keep: impl FnMut(T) -> NativeResult<bool>,
) -> NativeResult<()> {
    let mut failure = None;
    values.retain(|value| {
        if failure.is_some() {
            return true;
        }
        match keep(value.clone()) {
            Ok(keep) => keep,
            Err(error) => {
                failure = Some(error);
                true
            }
        }
    });
    failure.map_or(Ok(()), Err)
}

fn dedup<T: Clone>(
    values: &mut Vec<T>,
    mut equal: impl FnMut(T, T) -> NativeResult<bool>,
) -> NativeResult<()> {
    let mut failure = None;
    values.dedup_by(|next, previous| {
        if failure.is_some() {
            return false;
        }
        match equal(previous.clone(), next.clone()) {
            Ok(equal) => equal,
            Err(error) => {
                failure = Some(error);
                false
            }
        }
    });
    failure.map_or(Ok(()), Err)
}

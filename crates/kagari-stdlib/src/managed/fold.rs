//! A bounded value-vector fold using the common call slots and callback seam.

use crate::managed::vector::{VectorMut, VectorStorage};
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::NativeResult,
        managed::{
            call::{CallRegion, NativeCall},
            payload::NativeObject,
            slots::{Local, Object, Plain},
        },
    },
};

/// Fold projected u64 values with collecting, synchronously reentrant callbacks.
///
/// This proof consumes a revision-checked value vector, not the complete
/// Iterable protocol. Projection runs inside a short region; all loans end before
/// invoking the reducer. Source edits are detected at the next element read.
pub fn fold<'call, T: Plain>(
    call: &mut NativeCall<'call>,
    values: Local<'call, Object<NativeObject<VectorStorage<T>>>>,
    initial: u64,
    project: impl for<'gc> Fn(&CallRegion<'_, 'gc>, T::View<'gc>) -> NativeResult<u64>,
    reducer: &mut impl FnMut(&mut NativeCall<'call>, u64, u64) -> NativeResult<u64>,
) -> NativeResult<u64> {
    let accumulator = call.local::<u64>()?;
    let item = call.local::<u64>()?;
    call.region(|r| r.write(accumulator, initial))?;
    let revision = call.region(|r| {
        let object = r.read(values)?;
        let vector = VectorMut::from_object(r, object)?.readable();
        vector.revision(r)
    })?;
    let mut index = 0;
    loop {
        let present = call.region(|r| {
            let object = r.read(values)?;
            let values = VectorMut::from_object(r, object)?.readable();
            if values.revision(r)? != revision {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "collection changed during iteration",
                ));
            }
            let Some(object) = values.get(r, index)? else {
                return Ok(false);
            };
            let value = project(r, object)?;
            r.write(item, value)?;
            Ok(true)
        })?;
        if !present {
            break;
        }
        call.invoke_into(reducer, (accumulator, item), accumulator)?;
        index += 1;
    }
    call.read_scalar(accumulator)
}

//! Nominal sequence storage factories and checked contiguous Native element views.
use crate::{
    error::RuntimeError,
    gc::arrays::storage::PackedStorage,
    native::{
        binding::NativeResult,
        scalar::NativeScalar,
        storage::{NativePayload, NativeStorage},
        storage_type::StorageType,
    },
    resource::HeapCharge,
    value::Value,
};
mod allocation;

use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};
use std::{any::Any, sync::Arc};

#[derive(Debug)]
pub(crate) struct SequencePayload {
    pub(crate) element: Ty,
    pub(crate) contract: Arc<StorageType>,
    pub(crate) values: PackedStorage,
    pub(crate) detached: bool,
}

impl NativePayload for SequencePayload {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        for value in self.values.traced() {
            visit(value);
        }
    }

    fn charge(&self) -> NativeResult<HeapCharge> {
        Ok(HeapCharge::new(
            self.values.len(),
            self.values.retained_bytes(),
        ))
    }
}

impl NativeStorage {
    pub fn sequence(element: usize) -> Self {
        Self::with_layout(NativeStorageLayout::Sequence { element }, move |context| {
            let item = match context.ty() {
                Ty::NativeObject(nominal) => nominal.arguments.get(element),
                _ => None,
            }
            .ok_or_else(|| RuntimeError::module_validation("sequence element type"))?;
            Ok(SequencePayload {
                detached: false,
                element: item.clone(),
                contract: context.element_contract(element)?,
                values: PackedStorage::empty(item),
            })
        })
    }
}

mod sealed {
    pub trait Element {}
}

/// Supported contiguous scalar elements. Views borrow one buffer for the closure.
///
/// Script `i64`/`u64` buffers use Rust `i64`/`u64` elements on every target.
/// Rust `isize`/`usize` scalar conveniences do not support zero-copy element views.
pub trait NativeElement: NativeScalar + sealed::Element {
    #[doc(hidden)]
    fn slice(storage: &dyn Any) -> Option<&[Self]>;

    #[doc(hidden)]
    fn slice_mut(storage: &mut dyn Any) -> Option<&mut [Self]>;
}

macro_rules! elements {
    ($($variant:ident:$rust:ty),+) => { $(
        impl sealed::Element for $rust {}
        impl NativeElement for $rust {
            fn slice(storage: &dyn Any) -> Option<&[Self]> {
                match storage.downcast_ref::<PackedStorage>()? {
                    PackedStorage::$variant(values) => Some(values), _ => None,
                }
            }

            fn slice_mut(storage: &mut dyn Any) -> Option<&mut [Self]> {
                match storage.downcast_mut::<PackedStorage>()? {
                    PackedStorage::$variant(values) => Some(values), _ => None,
                }
            }
        }
    )+ };
}

elements!(Unit:(), Bool:bool, Char:char, I8:i8, I16:i16, I32:i32, I64:i64,
    U8:u8, U16:u16, U32:u32, U64:u64, F32:f32, F64:f64);

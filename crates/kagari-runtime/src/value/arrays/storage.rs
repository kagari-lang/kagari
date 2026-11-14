//! Fixed primitive buffers and scoped/traced roles for managed array elements.
use crate::value::{
    carriers::{ScopedValue, StoredValue},
    properties::ValueProperties,
    scalar::elements::Primitive,
};
use kagari_gc::{
    access::{Init, Mutation, Scope},
    error::GcError,
    object::{Gc, GcObject},
    scan::{EdgeVisitor, ScanBudget, ScanStatus},
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::ops::Range;

fn length(current: usize, additional: usize) -> Result<usize, GcError> {
    let result = current
        .checked_add(additional)
        .ok_or(GcError::Limit("array length"))?;
    // Runtime's machine-sized logical occupancy also includes the array itself.
    result
        .checked_add(1)
        .ok_or(GcError::Limit("array length"))?;
    Ok(result)
}

macro_rules! storage {
    ($($variant:ident:$rust:ty),+ $(,)?) => {
        /// Persistent cells belong to their fully traced array provider only.
        #[derive(Debug)]
        pub(crate) enum PackedStorage {
            $($variant(Vec<$rust>),)+
            Traced(Vec<StoredValue>),
        }

        /// Preparation is collection-free and branded. Scalar Vecs transfer
        /// without copying; managed targets become edges only during commit.
        #[derive(Debug)]
        pub(crate) struct ArrayValues<'gc> {
            buffer: ScopedBuffer<'gc>,
        }

        #[derive(Debug)]
        enum ScopedBuffer<'gc> {
            $($variant(Vec<$rust>),)+
            Traced(Vec<ScopedValue<'gc>>),
        }

        #[derive(Debug, Clone, Copy)]
        pub(super) enum Element<'gc> {
            $($variant($rust),)+
            Traced(ScopedValue<'gc>),
        }

        impl<'gc> ArrayValues<'gc> {
            pub(crate) fn empty(element: &Ty) -> Self {
                Self { buffer: match element {
                    $(Ty::Builtin(BuiltinType::$variant) => ScopedBuffer::$variant(Vec::new()),)+
                    _ => ScopedBuffer::Traced(Vec::new()),
                } }
            }

            pub(crate) fn len(&self) -> usize {
                match &self.buffer { $(ScopedBuffer::$variant(values) => values.len(),)+ ScopedBuffer::Traced(values) => values.len() }
            }

            pub(crate) fn reserve(&mut self, additional: usize) -> Result<(), GcError> {
                length(self.len(), additional)?;
                match &mut self.buffer { $(ScopedBuffer::$variant(values) => values.try_reserve_exact(additional)?,)+ ScopedBuffer::Traced(values) => values.try_reserve_exact(additional)? }
                Ok(())
            }

            pub(crate) fn push(&mut self, cx: &Scope<'gc>, value: ScopedValue<'gc>) -> Result<(), GcError> {
                if !ValueProperties::of(cx, value)?.storable { return Err(GcError::Representation); }
                self.push_admitted(value)
            }

            fn push_admitted(&mut self, value: ScopedValue<'gc>) -> Result<(), GcError> {
                length(self.len(), 1)?;
                match &mut self.buffer {
                    $(ScopedBuffer::$variant(values) => {
                        let value = <$rust>::decode(value)?;
                        values.try_reserve(1)?;
                        values.push(value);
                    },)+
                    ScopedBuffer::Traced(values) => { values.try_reserve(1)?; values.push(value); }
                }
                Ok(())
            }

            pub(crate) fn repeat(&mut self, cx: &Scope<'gc>, value: ScopedValue<'gc>, count: usize) -> Result<(), GcError> {
                if !ValueProperties::of(cx, value)?.storable { return Err(GcError::Representation); }
                let end = length(self.len(), count)?;
                match &mut self.buffer {
                    $(ScopedBuffer::$variant(values) => {
                        let value = <$rust>::decode(value)?;
                        values.try_reserve(count)?;
                        values.resize(end, value);
                    },)+
                    ScopedBuffer::Traced(values) => { values.try_reserve(count)?; values.resize(end, value); }
                }
                Ok(())
            }

            pub(super) fn has_type(&self, ty: &Ty) -> bool {
                match ty {
                    $(Ty::Builtin(BuiltinType::$variant) => matches!(&self.buffer, ScopedBuffer::$variant(_)),)+
                    _ => matches!(&self.buffer, ScopedBuffer::Traced(_)),
                }
            }

            pub(super) fn matches(&self, target: &PackedStorage) -> bool {
                matches!((&self.buffer, target), $((ScopedBuffer::$variant(_), PackedStorage::$variant(_)) |)+ (ScopedBuffer::Traced(_), PackedStorage::Traced(_)))
            }

            /// # Safety
            /// The resulting cells belong to the initializer's returned fully
            /// traced array. Protected inputs cannot be exposed outside it.
            pub(super) unsafe fn initialize(self, init: &mut Init<'_>) -> Result<PackedStorage, GcError> {
                Ok(match self.buffer {
                    $(ScopedBuffer::$variant(values) => PackedStorage::$variant(values),)+
                    ScopedBuffer::Traced(values) => {
                        let mut fields = Vec::new();
                        fields.try_reserve_exact(values.len())?;
                        for value in values {
                            // SAFETY: inherited initialized-array owner contract.
                            fields.push(unsafe { StoredValue::initialize(init, value)? });
                        }
                        PackedStorage::Traced(fields)
                    }
                })
            }

            /// Stage every new edge before touching the destination. A failure
            /// drops inert scratch; successful return must be committed in owner.
            ///
            /// # Safety
            /// Targets share mutation's scope; install the returned buffer only
            /// into this owner's fully scanned storage before mutation ends.
            pub(super) unsafe fn publish<T: GcObject>(self, mutation: &mut Mutation<'_, T>) -> Result<PackedStorage, GcError> {
                Ok(match self.buffer {
                    $(ScopedBuffer::$variant(values) => PackedStorage::$variant(values),)+
                    ScopedBuffer::Traced(values) => {
                        let mut fields = Vec::new();
                        fields.try_reserve_exact(values.len())?;
                        for value in values {
                            // SAFETY: caller commits the complete owner buffer.
                            fields.push(unsafe { StoredValue::publish_field(mutation, value)? });
                        }
                        PackedStorage::Traced(fields)
                    }
                })
            }
        }

        impl PackedStorage {
            pub(crate) fn len(&self) -> usize {
                match self { $(Self::$variant(values) => values.len(),)+ Self::Traced(values) => values.len() }
            }

            pub(crate) fn retained_bytes(&self) -> usize {
                match self {
                    $(Self::$variant(values) => values.capacity().checked_mul(size_of::<$rust>()).expect("allocated array capacity"),)+
                    Self::Traced(values) => values.capacity().checked_mul(size_of::<StoredValue>()).expect("allocated array capacity"),
                }
            }

            pub(super) fn prepare<'gc>(&self, value: ScopedValue<'gc>) -> Result<Element<'gc>, GcError> {
                Ok(match self { $(Self::$variant(_) => Element::$variant(<$rust>::decode(value)?),)+ Self::Traced(_) => Element::Traced(value) })
            }

            /// # Safety
            /// This is mutation's exclusively borrowed, completely traced buffer.
            /// Input targets share its protected heap scope.
            pub(super) unsafe fn write<T: GcObject>(&mut self, mutation: &mut Mutation<'_, T>, index: usize, value: Element<'_>) -> Result<(), GcError> {
                if index >= self.len() { return Err(GcError::Index); }
                match (self, value) {
                    $((Self::$variant(values), Element::$variant(value)) => values[index] = value,)+
                    (Self::Traced(values), Element::Traced(value)) => {
                        // SAFETY: prepared replacement installed before return.
                        let value = unsafe { StoredValue::publish_field(mutation, value)? };
                        values[index] = value;
                    }
                    _ => return Err(GcError::Representation),
                }
                Ok(())
            }

            fn scalar<'gc>(&self, index: usize) -> Option<ScopedValue<'gc>> {
                match self { $(Self::$variant(values) => values.get(index).copied().map(Primitive::encode),)+ Self::Traced(_) => None }
            }

            fn append_scalar(&self, range: Range<usize>, output: &mut ArrayValues<'_>) -> Result<bool, GcError> {
                if range.start > range.end || range.end > self.len() { return Err(GcError::Index); }
                length(output.len(), range.len())?;
                match (self, &mut output.buffer) {
                    $((Self::$variant(values), ScopedBuffer::$variant(target)) => {
                        target.try_reserve(range.len())?;
                        target.extend_from_slice(&values[range]);
                        Ok(true)
                    },)+
                    (Self::Traced(_), ScopedBuffer::Traced(_)) => Ok(false),
                    _ => Err(GcError::Representation),
                }
            }

            /// Fully admitted shape/range; moving staged carriers cannot fail.
            pub(super) fn overwrite(&mut self, start: usize, source: Self) {
                let end = start + source.len();
                match (self, source) {
                    $((Self::$variant(values), Self::$variant(source)) => values[start..end].copy_from_slice(&source),)+
                    (Self::Traced(values), Self::Traced(source)) => {
                        for (slot, value) in values[start..end].iter_mut().zip(source) { *slot = value; }
                    }
                    _ => unreachable!("prepared fixed-array storage kind"),
                }
            }


        }
    };
}

storage!(Unit:(), Bool:bool, Char:char, I8:i8, I16:i16, I32:i32, I64:i64,
    U8:u8, U16:u16, U32:u32, U64:u64, F32:f32, F64:f64);

impl PackedStorage {
    pub(super) fn get<'gc, T: GcObject>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, T>,
        select: fn(&T) -> &Self,
        index: usize,
    ) -> Result<ScopedValue<'gc>, GcError> {
        if let Some(value) = cx.read(owner, |owner| select(owner).scalar(index)) {
            return Ok(value);
        }
        StoredValue::get_field(cx, owner, |owner| match select(owner) {
            Self::Traced(values) => values.get(index),
            _ => None,
        })
    }

    /// Copy from an actual retained array with the provider's storable-field
    /// invariant. The same closed scope excludes callbacks and source mutation.
    ///
    /// # Safety
    /// select names this admitted owner's fully initialized array buffer, whose
    /// fields were admitted as storable; no independent detached carrier is valid.
    pub(super) unsafe fn copy_into<'gc, T: GcObject>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, T>,
        select: fn(&T) -> &Self,
        range: Range<usize>,
        output: &mut ArrayValues<'gc>,
    ) -> Result<(), GcError> {
        if cx.read(owner, |owner| {
            select(owner).append_scalar(range.clone(), output)
        })? {
            return Ok(());
        }
        for index in range {
            output.push_admitted(Self::get(cx, owner, select, index)?)?;
        }
        Ok(())
    }

    /// # Safety
    /// The visitor has admitted this buffer's retained, fully initialized owner.
    pub(super) unsafe fn scan(
        &self,
        next: &mut usize,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if let Self::Traced(values) = self {
            while let Some(value) = values.get(*next) {
                if !budget.take(1) {
                    return Ok(ScanStatus::Pending);
                }
                // SAFETY: inherited initialized-buffer owner contract.
                if unsafe { value.scan(budget, visitor)? } == ScanStatus::Pending {
                    return Ok(ScanStatus::Pending);
                }
                *next += 1;
            }
        }
        Ok(ScanStatus::Complete)
    }
}

#[cfg(test)]
mod tests;

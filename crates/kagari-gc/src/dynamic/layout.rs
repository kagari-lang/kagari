//! Checked layout construction and resumable scanning of fixed reference slots.

use crate::{
    access::Access,
    dynamic::{
        DynamicLayout, DynamicShape, Field, FieldType, ReferenceField, Scalar, ScalarField, sealed,
    },
    error::GcError,
    object::ObjectRef,
    scan::{EdgeVisitor, ScanBudget, ScanStatus},
};
use std::{alloc::Layout, cmp::Reverse, marker::PhantomData, ptr::NonNull};

macro_rules! scalar_types {
    ($($ty:ty => $kind:ident),+ $(,)?) => {
        $(impl sealed::Sealed for $ty {}

        impl Scalar for $ty {
            const FIELD_TYPE: FieldType = FieldType::$kind;
        })+

        impl FieldType {
            fn layout(self) -> Layout {
                match self {
                    $(Self::$kind => Layout::new::<$ty>(),)+
                    Self::Reference => Layout::new::<Option<ObjectRef>>(),
                }
            }
        }
    };
}

scalar_types!(bool => Bool, char => Char, i8 => I8, u8 => U8, i16 => I16,
    u16 => U16, i32 => I32, u32 => U32, i64 => I64, u64 => U64,
    i128 => I128, u128 => U128, f32 => F32, f64 => F64);

impl Access<'_> {
    /// Register an immutable physical record layout. Checked composition rejects
    /// overflow; field selectors subsequently verify index and scalar kind.
    /// Physical fields are ordered by descending alignment, with declaration order
    /// breaking ties. Selector indices always refer to the input field order.
    /// The caller owns semantic interning; registration creates a fresh identity.
    pub fn register_dynamic(&mut self, fields: &[FieldType]) -> Result<DynamicLayout, GcError> {
        self.heap.marking.check()?;
        let shape = DynamicShape::new(fields)?;
        let descriptor = self.heap.physical.descriptors.register_dynamic(shape)?;
        self.heap.workspace.reserve(descriptor.state_layout)?;
        Ok(DynamicLayout {
            heap: self.heap.id,
            descriptor,
        })
    }
}

impl DynamicLayout {
    /// Prepare one scalar field; reject an out-of-range index or differing kind.
    pub fn scalar<V: Scalar>(&self, index: usize) -> Result<ScalarField<V>, GcError> {
        let field = self
            .descriptor
            .shape()?
            .fields
            .get(index)
            .ok_or(GcError::Index)?;
        if field.kind != V::FIELD_TYPE {
            return Err(GcError::Representation);
        }
        Ok(ScalarField {
            layout: self.clone(),
            offset: field.offset,
            marker: PhantomData,
        })
    }

    /// Prepare one reference field, rejecting scalar fields and invalid indices.
    pub fn reference(&self, index: usize) -> Result<ReferenceField, GcError> {
        let field = self
            .descriptor
            .shape()?
            .fields
            .get(index)
            .ok_or(GcError::Index)?;
        if field.kind != FieldType::Reference {
            return Err(GcError::Representation);
        }
        Ok(ReferenceField {
            layout: self.clone(),
            offset: field.offset,
        })
    }

    pub fn field_count(&self) -> usize {
        self.descriptor
            .shape()
            .expect("registered dynamic layout")
            .fields
            .len()
    }

    /// Inline data bytes, excluding the GC header and physical page rounding.
    pub fn size(&self) -> usize {
        self.descriptor
            .shape()
            .expect("registered dynamic layout")
            .layout
            .size()
    }

    pub fn alignment(&self) -> usize {
        self.descriptor
            .shape()
            .expect("registered dynamic layout")
            .layout
            .align()
    }
}

impl DynamicShape {
    fn new(kinds: &[FieldType]) -> Result<Self, GcError> {
        let mut layout = Layout::from_size_align(0, 1).expect("empty layout");
        let mut fields = Vec::new();
        let mut references = Vec::new();
        let mut order = Vec::new();
        fields.try_reserve_exact(kinds.len())?;
        order.try_reserve_exact(kinds.len())?;
        references.try_reserve_exact(
            kinds
                .iter()
                .filter(|kind| **kind == FieldType::Reference)
                .count(),
        )?;
        for (index, &kind) in kinds.iter().enumerate() {
            fields.push(Field { kind, offset: 0 });
            order.push(index);
        }
        // Sizes are multiples of power-of-two alignment. Descending alignment
        // avoids inter-field padding; logical indices remain stable in fields.
        // Explicit index tie-breaking makes the allocation-free sort deterministic.
        order.sort_unstable_by_key(|&index| (Reverse(kinds[index].layout().align()), index));
        for index in order {
            let field = &mut fields[index];
            let (next, offset) = layout
                .extend(field.kind.layout())
                .map_err(|_| GcError::Limit("dynamic field layout"))?;
            layout = next;
            field.offset = offset;
        }
        for field in &fields {
            if field.kind == FieldType::Reference {
                references.push(field.offset);
            }
        }
        Ok(Self {
            layout: layout.pad_to_align(),
            fields: fields.into_boxed_slice(),
            references: references.into_boxed_slice(),
        })
    }

    /// Caller owns a reserved allocation of this exact shape. Every supported
    /// scalar admits all-zero bits; reference slots are then initialized as typed
    /// None values. No partially initialized object is published to the collector.
    pub(crate) unsafe fn initialize(&self, pointer: NonNull<u8>) {
        unsafe {
            pointer.as_ptr().write_bytes(0, self.layout.size());
            for &offset in &self.references {
                pointer
                    .as_ptr()
                    .add(offset)
                    .cast::<Option<ObjectRef>>()
                    .write(None);
            }
        }
    }

    /// Caller retains this initialized record and its matching cursor workspace.
    /// Coordinates are immutable; no field update can invalidate the next index.
    pub(crate) unsafe fn scan(
        &self,
        pointer: NonNull<u8>,
        next: &mut usize,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while *next < self.references.len() {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let offset = self.references[*next];
            // SAFETY: layout construction proves alignment/bounds; initialization
            // and safe field writes preserve Option<ObjectRef> and its provenance.
            let target = unsafe {
                pointer
                    .as_ptr()
                    .add(offset)
                    .cast::<Option<ObjectRef>>()
                    .read()
            };
            if let Some(target) = target
                && unsafe { visitor.try_visit_reference(target, budget)? } == ScanStatus::Pending
            {
                return Ok(ScanStatus::Pending);
            }
            *next += 1;
        }
        Ok(ScanStatus::Complete)
    }
}

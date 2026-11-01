//! Immutable runtime-defined records with checked scalar and reference fields.
//!
//! Layouts contain physical facts only, never script names, modules or nominal
//! types. Registration computes offsets; callers cannot supply unchecked bytes.
//! All fields start at zero/false/NUL or None. References may point to dynamic or
//! Rust objects and use the same roots, publication and collection protocols.
//!
//! ```
//! use kagari_gc::{dynamic::DynamicLayoutBuilder, error::GcError, heap::Heap};
//! fn example() -> Result<(), GcError> {
//!     let mut heap = Heap::default();
//!     let mut builder = DynamicLayoutBuilder::new();
//!     let hp = builder.scalar::<i64>();
//!     let target = builder.reference();
//!     let layout = heap.access()?.register_dynamic(builder)?;
//!     let hp = layout.scalar(hp)?;
//!     let target = layout.reference(target)?;
//!     let root = heap.scope(|cx| {
//!         let player = cx.alloc_dynamic(&layout)?;
//!         let enemy = cx.alloc_dynamic(&layout)?;
//!         hp.set(cx, enemy, 100)?;
//!         target.set(cx, player, Some(enemy))?;
//!         cx.retain(player)
//!     })?;
//!     heap.collect()?;
//!     heap.scope(|cx| {
//!         let player = cx.get(&root)?;
//!         let enemy = target.get(cx, player)?.unwrap();
//!         assert_eq!(hp.get(cx, enemy)?, 100);
//!         Ok(())
//!     })?;
//!     Ok(())
//! }
//! example().unwrap();
//! ```

mod access;
mod builder;
mod layout;

use crate::descriptor::Descriptor;
use std::{alloc::Layout, marker::PhantomData, sync::Arc};

/// Physical field kinds. Reference slots are nullable and accept any managed
/// representation; language-level target types and required fields belong to the VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Bool,
    Char,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    I128,
    U128,
    F32,
    F64,
    Reference,
}

/// The closed set of directly stored, zero-initializable scalar representations.
/// Custom Rust types use GcObject rather than supplying arbitrary byte codecs.
pub trait Scalar: sealed::Sealed + Copy + Send + 'static {
    const FIELD_TYPE: FieldType;
}

mod sealed {
    pub trait Sealed {}
}

/// A physical schema under construction. Adding fields returns typed keys; register
/// the completed builder once with Access::register_dynamic. Names belong to the
/// caller's metadata or named accessor struct, never to the collector.
/// Builders cannot be cloned; each one establishes a distinct field-key identity.
#[derive(Debug, Default)]
pub struct DynamicLayoutBuilder {
    identity: Arc<()>,
    fields: Vec<FieldType>,
}

/// A scalar field declared in one builder, before its physical offset is known.
/// Cloning retains only schema identity, not a registered layout or any objects.
/// Bind with DynamicLayout::scalar; keys cannot be constructed from raw indices.
#[derive(Debug, Clone)]
pub struct ScalarKey<V: Scalar> {
    field: FieldKey,
    marker: PhantomData<fn() -> V>,
}

/// A nullable reference field declared in one builder. Bind with
/// DynamicLayout::reference after registration. Cloning retains schema identity
/// only; it does not keep the layout or any managed object alive.
#[derive(Debug, Clone)]
pub struct ReferenceKey {
    field: FieldKey,
}

#[derive(Debug, Clone)]
struct FieldKey {
    identity: Arc<()>,
    index: usize,
}

/// A heap-local immutable descriptor lease. Clone retains metadata, not objects.
/// Equal-shaped independent registrations still have distinct layout identities.
#[derive(Debug, Clone)]
pub struct DynamicLayout {
    pub(crate) heap: u64,
    pub(crate) descriptor: Arc<Descriptor>,
}

/// A checked scalar offset pinned to one exact registered layout.
#[derive(Debug, Clone)]
pub struct ScalarField<V: Scalar> {
    layout: DynamicLayout,
    offset: usize,
    marker: PhantomData<fn() -> V>,
}

/// A checked nullable reference offset pinned to one exact registered layout.
#[derive(Debug, Clone)]
pub struct ReferenceField {
    layout: DynamicLayout,
    offset: usize,
}

#[derive(Debug)]
pub(crate) struct DynamicShape {
    identity: Arc<()>,
    pub(crate) layout: Layout,
    fields: Box<[Field]>,
    pub(crate) references: Box<[usize]>,
}

#[derive(Debug)]
struct Field {
    kind: FieldType,
    offset: usize,
}

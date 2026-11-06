//! Schema construction assigns opaque field keys before physical layout packing.

use crate::dynamic::{DynamicLayoutBuilder, FieldKey, FieldType, ReferenceKey, Scalar, ScalarKey};
use std::marker::PhantomData;

impl DynamicLayoutBuilder {
    /// Start a distinct, empty schema. Moving the builder preserves its keys.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a scalar field and return its typed key directly.
    /// Storage growth follows ordinary Vec allocation behavior.
    ///
    /// A scalar key cannot bind a different Rust scalar type:
    /// ```compile_fail,E0308
    /// use kagari_gc::dynamic::{DynamicLayoutBuilder, ScalarKey};
    /// let mut builder = DynamicLayoutBuilder::new();
    /// let key: ScalarKey<i64> = builder.scalar::<u64>();
    /// ```
    pub fn scalar<V: Scalar>(&mut self) -> ScalarKey<V> {
        ScalarKey {
            field: self.push(V::FIELD_TYPE),
            marker: PhantomData,
        }
    }

    /// Append a nullable managed-reference field and return its key directly.
    /// Storage growth follows ordinary Vec allocation behavior.
    /// Reference keys cannot bind scalar accessors:
    /// ```compile_fail,E0308
    /// use kagari_gc::{dynamic::DynamicLayoutBuilder, heap::Heap};
    /// let mut heap = Heap::default();
    /// let mut builder = DynamicLayoutBuilder::new();
    /// let key = builder.reference();
    /// let layout = heap.access().unwrap().register_dynamic(builder).unwrap();
    /// let field = layout.scalar(key).unwrap();
    /// ```
    pub fn reference(&mut self) -> ReferenceKey {
        ReferenceKey {
            field: self.push(FieldType::Reference),
        }
    }

    fn push(&mut self, kind: FieldType) -> FieldKey {
        let index = self.fields.len();
        self.fields.push(kind);
        FieldKey {
            identity: self.identity.clone(),
            index,
        }
    }
}

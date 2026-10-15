//! Manual leaf schema shared by the external derive and internal rollback cases.
use super::{DeclarationCatalog, KagariType, NativeResult, Type, TypeRegistration, ValueField};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Leaf {
    pub(super) value: i32,
}

impl KagariType for Leaf {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(cx: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        cx.structure::<Self>("Leaf", |_| Ok(vec![ValueField::new("value", Type::i32())]))
    }
}

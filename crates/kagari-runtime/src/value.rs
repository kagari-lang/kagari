pub(crate) mod aggregates;
pub(crate) mod arrays;
pub mod carriers;
pub(crate) mod cells;
pub(crate) mod closures;
pub mod ephemeral;
pub(crate) mod host_values;
pub(crate) mod interfaces;
pub mod kind;
pub(crate) mod native;
pub mod place;
pub(crate) mod properties;
pub(crate) mod scalar;
pub(crate) mod strings;
pub mod view;

use crate::{
    host::{FrameHostBorrowToken, scoped::ScopedHostView},
    module::EnumVariantRef,
    value::ephemeral::EphemeralValueId,
};

/// A declared enum layout used for checked Host construction. Payload storage
/// belongs to the actual EnumObject provider, never an unrooted value snapshot.
#[derive(Debug, Clone, PartialEq)]
pub enum EnumTag {
    Declared(EnumVariantRef),
}

impl EnumTag {
    pub fn type_name(&self) -> &str {
        match self {
            Self::Declared(layout) => layout
                .module()
                .definition_name(layout.layout().declaration)
                .expect("enum declaration"),
        }
    }

    pub fn variant_name(&self) -> &str {
        match self {
            Self::Declared(layout) => layout
                .module()
                .definition_name(layout.variant().declaration)
                .expect("variant declaration"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EphemeralValue {
    HostRef(FrameHostBorrowToken),
    HostMut(FrameHostBorrowToken),
    Runtime(EphemeralValueId),
    HostView(ScopedHostView),
}

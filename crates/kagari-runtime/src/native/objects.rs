//! Retained script objects and generation-pinned, prepared field access.
pub(crate) mod cache;
pub mod construction;
mod conversion;
pub mod fields;
mod methods;

use crate::{
    Runtime,
    frame::types::arguments::TypeArgument,
    module::{
        ExecutableModule, StructLayoutRef, managed::source::RevisionSource, retention::ProgramLease,
    },
    native::{binding::NativeResult, typed::NativeContext},
    value::carriers::RootedValue,
};
use std::{marker::PhantomData, sync::Arc};

/// A dynamic object still has an exact installed nominal type. The marker only
/// means that Rust does not supply a static schema for that type.
#[derive(Debug)]
pub struct Dynamic;

/// A host lease, never a Rust reference into movable script storage.
#[derive(Debug)]
pub struct Object<S = Dynamic> {
    root: RootedValue,
    object_type: ObjectType,
    layout: StructLayoutRef,
    schema: PhantomData<fn() -> S>,
}

impl<S> Clone for Object<S> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            object_type: self.object_type.clone(),
            layout: self.layout.clone(),
            schema: PhantomData,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ObjectType(pub(crate) Arc<TypeRecord>);

#[derive(Debug)]
pub(crate) struct TypeRecord {
    layout: StructLayoutRef,
    argument: TypeArgument,
    public: bool,
    program: ProgramLease,
}

impl ObjectType {
    #[cfg(feature = "serde")]
    pub(crate) fn field_names(&self) -> Vec<String> {
        self.0
            .layout
            .layout()
            .fields
            .iter()
            .map(|field| field.name.clone())
            .collect()
    }

    pub fn type_argument(&self) -> &TypeArgument {
        &self.0.argument
    }

    pub(crate) fn layout(&self) -> &StructLayoutRef {
        &self.0.layout
    }

    pub fn owner(&self) -> &ExecutableModule {
        self.0.layout.module()
    }

    pub(crate) fn origin(&self) -> RevisionSource<'_> {
        RevisionSource::Host(self.0.program.revision())
    }

    pub(crate) fn validate(&self, runtime: &Runtime) -> NativeResult<()> {
        runtime.validate_executable_module(self.owner())
    }
}

impl<S> Object<S> {
    pub fn object_type(&self) -> &ObjectType {
        &self.object_type
    }

    pub(crate) fn validate(&self, cx: &mut NativeContext<'_>) -> NativeResult<()> {
        cx.conversion.check_struct_root(&self.root, &self.layout)
    }
}

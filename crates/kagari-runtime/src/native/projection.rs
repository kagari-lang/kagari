//! Local Rust layout proofs generated alongside finite struct schemas.
#[cfg(test)]
mod tests;
use crate::{
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, context::ConversionContext},
        types::Type,
        value_handle::ScriptValue,
    },
    value::Value,
};
use std::{
    any::{Any, TypeId},
    fmt,
};

/// A generated direct field address calculation. Portable declarations contain
/// names and semantic types; this record and its function stay in the Rust host.
#[derive(Clone)]
pub struct GeneratedField {
    pub(crate) name: String,
    pub(crate) root: TypeId,
    pub(crate) target: TypeId,
    pub(crate) target_type: fn(&DeclarationCatalog) -> NativeResult<Type>,
    pub(crate) project: unsafe fn(*mut ()) -> *mut (),
    pub(crate) snapshot:
        unsafe fn(*mut (), &mut ConversionContext<'_>, &TypeArgument) -> NativeResult<ScriptValue>,
    pub(crate) decode:
        fn(&mut ConversionContext<'_>, &TypeArgument, &Value) -> NativeResult<Box<dyn Any>>,
    pub(crate) replace: unsafe fn(*mut (), Box<dyn Any>) -> Box<dyn Any>,
}

impl fmt::Debug for GeneratedField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeneratedField")
            .field("name", &self.name)
            .field("root", &self.root)
            .field("target", &self.target)
            .field("projector", &self.project)
            .finish_non_exhaustive()
    }
}

impl GeneratedField {
    /// Construct derive output for a direct, initialized, aligned inline field.
    /// This is the macro support boundary, not a schema-to-layout inference API.
    ///
    /// # Safety
    /// For every valid live pointer to T, `project` must compute the address of
    /// its named F field using raw field projection without creating a reference
    /// to T or any ancestor. The field must be an ordinary inline Rust place,
    /// never packed, union/variant-dependent, indexed, dereferenced or aliased
    /// storage. Its address remains valid while T is reserved by the host scope.
    #[doc(hidden)]
    pub unsafe fn new<T: 'static, F: FromKagari + 'static>(
        name: &str,
        project: unsafe fn(*mut ()) -> *mut (),
    ) -> Self {
        Self {
            name: name.into(),
            root: TypeId::of::<T>(),
            target: TypeId::of::<F>(),
            target_type: F::kagari_type,
            project,
            snapshot: snapshot::<F>,
            decode: decode::<F>,
            replace: replace::<F>,
        }
    }
}

unsafe fn snapshot<F: FromKagari>(
    pointer: *mut (),
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
) -> NativeResult<ScriptValue> {
    // SAFETY: The caller holds a checked shared path loan with exact F identity.
    unsafe { (&*pointer.cast::<F>()).snapshot_field(cx, expected) }
}

fn decode<F: FromKagari + 'static>(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    value: &Value,
) -> NativeResult<Box<dyn Any>> {
    // SAFETY: The field writer supplies a live value retained by its execution frame.
    // Conversion finishes before the host loan or replacement starts.
    unsafe { cx.decode_transport::<F>(expected, value) }
        .map(|value| Box::new(value) as Box<dyn Any>)
}

unsafe fn replace<F: 'static>(pointer: *mut (), value: Box<dyn Any>) -> Box<dyn Any> {
    let value = *value
        .downcast::<F>()
        .expect("generated field decoder identity");
    // SAFETY: The caller acquired the exclusive exact-F path after decoding.
    // Return the displaced value so its destructor runs after releasing the loan.
    Box::new(unsafe { pointer.cast::<F>().replace(value) })
}

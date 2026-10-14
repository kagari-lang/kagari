//! Local Rust layout proofs generated alongside finite struct schemas.
#[cfg(test)]
mod tests;
use crate::native::{
    binding::NativeResult, catalog::DeclarationCatalog, conversion::KagariType, types::Type,
};
use std::{any::TypeId, fmt};

/// A generated direct field address calculation. Portable declarations contain
/// names and semantic types; this record and its function stay in the Rust host.
#[derive(Clone)]
pub struct GeneratedField {
    pub(crate) name: String,
    pub(crate) root: TypeId,
    pub(crate) target: TypeId,
    pub(crate) target_type: fn(&DeclarationCatalog) -> NativeResult<Type>,
    pub(crate) project: unsafe fn(*mut ()) -> *mut (),
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
    pub unsafe fn new<T: 'static, F: KagariType + 'static>(
        name: &str,
        project: unsafe fn(*mut ()) -> *mut (),
    ) -> Self {
        Self {
            name: name.into(),
            root: TypeId::of::<T>(),
            target: TypeId::of::<F>(),
            target_type: F::kagari_type,
            project,
        }
    }
}

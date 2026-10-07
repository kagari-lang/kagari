//! Declared writeability of bindings and fields.

/// Source binding/field writeability; separate from the value's resolved type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Writeability {
    /// `val`: a non-reassignable binding or field.
    Val,
    /// `var`: a binding or field that permits assignment after checking.
    Var,
}

impl Writeability {
    /// Returns whether this is the `val` form.
    pub fn is_val(self) -> bool {
        matches!(self, Self::Val)
    }

    /// Returns whether this is the `var` form.
    pub fn is_var(self) -> bool {
        matches!(self, Self::Var)
    }
}

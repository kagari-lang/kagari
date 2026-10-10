//! Declared writeability of bindings and fields.

/// Declared rebinding/field-assignment policy, separate from value type and identity.
///
/// ```text
/// val a = value;        -> StmtKind::Binding { writeability: Val, ... }
/// var b = value;        -> StmtKind::Binding { writeability: Var, ... }
/// struct S { var x: i32, val y: i32 }
///                      -> Field(x).writeability = Var; Field(y).writeability = Val
/// fn consume(x: i32) {}     -> Param(x).writeability = Val (supplied by lowering)
/// ```
///
/// `Var` permits assignment only after type/access checks; `Val` prevents slot
/// rebinding. A `val` binding holding a shared object does not freeze that object
/// or other aliases. Local/field/parameter lowering records this policy, then
/// assignment checking consumes it; runtime borrowing has a separate owner.
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

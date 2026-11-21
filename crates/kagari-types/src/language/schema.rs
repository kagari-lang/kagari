//! Public value declarations consumed by core language operations.
use super::binding;
use crate::{
    declaration::{TypeDef, TypeDefKind, native::NativeTypeConstructor},
    ty::GenericParam,
};
use kagari_common::identity::{
    DefinitionPath,
    table::{DefinitionId, DefinitionTable},
};

/// A range constructor is a language type, not an external storage provider.
pub fn range_shape_valid(owner: DefinitionId, actual: &TypeDef, table: &DefinitionTable) -> bool {
    let Some(constructor @ NativeTypeConstructor::Range(kind)) =
        NativeTypeConstructor::from_definition(owner, table)
    else {
        return false;
    };
    actual.name == kind.name()
        && actual.kind == TypeDefKind::Native(constructor)
        && constructor.shape_valid(actual)
        && actual.bounds.is_empty()
        && actual
            .generic_params
            .iter()
            .enumerate()
            .all(|(i, p)| p.owner == owner && p.position == i)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreEnum {
    Option,
    Ordering,
    Result,
    Bound,
    ControlFlow,
    Infallible,
}

impl CoreEnum {
    pub const COMPARISON: [Self; 2] = [Self::Option, Self::Ordering];
    pub const OPERATIONS: [Self; 4] = [
        Self::Result,
        Self::Bound,
        Self::ControlFlow,
        Self::Infallible,
    ];

    pub fn declaration(self) -> DefinitionPath {
        match self {
            Self::Option => binding::option_declaration(),
            Self::Ordering => binding::ordering_declaration(),
            Self::Result => binding::result_declaration(),
            Self::Bound => binding::bound_declaration(),
            Self::ControlFlow => binding::control_flow_declaration(),
            Self::Infallible => binding::infallible_declaration(),
        }
    }

    /// Variant names declare ordered children. The linked verifier independently
    /// authenticates the IDs of materialized payload layouts.
    pub fn shape_valid(
        self,
        owner: DefinitionId,
        actual: &TypeDef,
        table: &DefinitionTable,
    ) -> bool {
        let path = self.declaration();
        let (arity, variants): (_, &[(&str, Option<usize>)]) = match self {
            Self::Option => (1, &[("Some", Some(0)), ("None", None)]),
            Self::Ordering => (0, &[("Less", None), ("Equal", None), ("Greater", None)]),
            Self::Result => (2, &[("Ok", Some(0)), ("Err", Some(1))]),
            Self::Bound => (
                1,
                &[
                    ("Included", Some(0)),
                    ("Excluded", Some(0)),
                    ("Unbounded", None),
                ],
            ),
            Self::ControlFlow => (2, &[("Break", Some(0)), ("Continue", Some(1))]),
            Self::Infallible => (0, &[]),
        };
        let parameter = |position| GenericParam { owner, position };
        table.lookup(&path) == Some(owner)
            && actual.name == path.path[0].name
            && actual.kind == TypeDefKind::Enum
            && actual.bounds.is_empty()
            && actual.fields.is_empty()
            && actual
                .generic_params
                .iter()
                .cloned()
                .eq((0..arity).map(parameter))
            && actual.variants.len() == variants.len()
            && actual
                .variants
                .iter()
                .zip(variants)
                .all(|(v, &(name, index))| {
                    v.name == name
                        && v.reports_failure == (self == Self::Result && name == "Err")
                        && v.payload
                            .iter()
                            .eq(index.map(|i| parameter(i).as_type()).iter())
                })
    }
}

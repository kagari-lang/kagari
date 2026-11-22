//! Bounded syntax/helper bridges and intrinsic scalar capabilities; ordinary libraries remain registered inputs.

pub mod surface;

use crate::types::{NominalType, TypeId};
use kagari_common::identity::{ModuleIdentity, table::DefinitionTable};
use kagari_types::{language::binding, range::RangeKind};

/// Small compiler-recognized helper surface; ordinary library functions use registered declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinFunction {
    /// The `type_of` helper surface for declared type metadata.
    TypeOf,
    /// The `get_field` helper surface, subject to checked field access.
    GetField,
    /// The `set_field` helper surface, subject to checked mutation access.
    SetField,
    /// The `set_index` helper surface, subject to checked index mutation.
    SetIndex,
    /// Project a canonical bound from a builtin range; only resolved inside core::ops.
    RangeBound { upper: bool },
    /// Reconstruct a reporting Result residual without changing its detached origin.
    ForwardErrOrigin,
}

impl BuiltinFunction {
    pub(crate) fn from_name(name: &str, module: Option<&ModuleIdentity>) -> Option<Self> {
        match name {
            "type_of" => Some(Self::TypeOf),
            "get_field" => Some(Self::GetField),
            "set_field" => Some(Self::SetField),
            "set_index" => Some(Self::SetIndex),
            "__forward_err_origin" if module == Some(&binding::result_declaration().module) => {
                Some(Self::ForwardErrOrigin)
            }
            "__range_start_bound" | "__range_end_bound"
                if module.is_some_and(|m| m.package.0 == "kagari-core" && m.path == ["ops"]) =>
            {
                Some(Self::RangeBound {
                    upper: name == "__range_end_bound",
                })
            }
            _ => None,
        }
    }
}

/// `RangeFull` has no element: its `Bound<T>` comes from the result context,
/// not from the unit used to encode the endpoint-free range representation.
pub(crate) fn projected_bound(
    receiver: &TypeId,
    expected: Option<&TypeId>,
    definitions: &DefinitionTable,
) -> Option<TypeId> {
    let TypeId::Range(item, kind) = receiver else {
        return None;
    };
    let declaration = definitions.lookup(&binding::bound_declaration())?;
    let element = if *kind == RangeKind::Full {
        let TypeId::Enum(result) = expected? else {
            return None;
        };
        let [element] = result.arguments.as_slice() else {
            return None;
        };
        if result.declaration != declaration || !result.associated_types.is_empty() {
            return None;
        }
        element
    } else {
        item.as_ref()
    };
    Some(TypeId::Enum(NominalType {
        declaration,
        arguments: vec![element.clone()],
        associated_types: Default::default(),
    }))
}

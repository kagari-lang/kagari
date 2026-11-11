use crate::{numeric::NumericOperation, representation::semantic_representation};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionTable;
use kagari_types::{
    declaration::verify::{concrete_type_valid, types_in_scope},
    language::binding,
    range::RangeKind,
    ty::Ty,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    Numeric(NumericOperation),
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    IdentityEq,
    IdentityNotEq,
    Lt,
    Gt,
    Le,
    Ge,
    AndAnd,
    OrOr,
}

/// Validate endpoint presence and physical types using the range's semantic type.
pub fn range_operands_valid(
    ty: &Ty,
    start: Option<ValueType>,
    end: Option<ValueType>,
    definitions: &DefinitionTable,
) -> bool {
    let Ty::Range(item, kind) = ty else {
        return false;
    };
    ty.within_wire_limits()
        && concrete_type_valid(ty, &Default::default(), definitions)
        && start == kind.has_start().then(|| semantic_representation(item))
        && end == kind.has_end().then(|| semantic_representation(item))
}

pub fn range_bound_valid(range: &Ty, bound: &Ty, table: &DefinitionTable) -> bool {
    let (Ty::Range(item, kind), Ty::Enum(nominal)) = (range, bound) else {
        return false;
    };
    range.within_wire_limits()
        && bound.within_wire_limits()
        && types_in_scope([range, bound], &[], &Default::default(), table)
        && binding::matches(&nominal.declaration, &binding::bound_declaration(), table)
        && nominal.arguments.len() == 1
        && nominal.associated_types.is_empty()
        && (*kind == RangeKind::Full || nominal.arguments[0] == **item)
}

/// Whether the static script type exposes object identity. Generic arguments
/// do not change a known object's category; interfaces hide that category.
pub fn object_identity_type(ty: &Ty) -> bool {
    matches!(ty, Ty::Struct(_) | Ty::NativeObject(_) | Ty::Array(_))
}

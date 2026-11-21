use crate::{
    numeric::NumericOperation,
    representation::semantic_representation,
    types::{PublicItem, type_contract},
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{ModuleIdentity, table::DefinitionTable};
use kagari_types::{
    declaration::verify::{concrete_type_valid, types_in_scope},
    language::{binding, schema::CoreEnum},
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

/// Portable authorization for the origin operation, independent of a source sidecar.
/// All three semantic slots and the reporting variant must survive artifact decoding.
pub fn result_origin_call_valid(
    module: &ModuleIdentity,
    items: &[PublicItem],
    operands: [Option<&Ty>; 2],
    output: Option<&Ty>,
    definitions: &DefinitionTable,
) -> bool {
    let [Some(original), Some(target)] = operands else {
        return false;
    };
    let result = binding::result_declaration();
    if module != &result.module
        || output != Some(target)
        || !binding::result_origin_carriers_valid(original, target, definitions)
    {
        return false;
    }
    let Some(id) = definitions.lookup(&result) else {
        return false;
    };
    let Some(schema) = type_contract(module, items, &id, definitions) else {
        return false;
    };
    CoreEnum::Result.shape_valid(id, schema, definitions)
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

//! Test-only comparison after validating and normalizing local node identities.

use crate::{
    hir::ids::{ExprId, FieldId, LoweringId},
    tests::common::import_facts,
    typeck::table::TypeTable,
};
use kagari_common::identity::{map::DefinitionContext, table::DefinitionTable};

impl TypeTable {
    pub(crate) fn assert_same_source_facts(
        &self,
        definitions: &DefinitionTable,
        other: &Self,
        other_definitions: &DefinitionTable,
        arena: LoweringId,
        other_arena: LoweringId,
    ) {
        let context = DefinitionContext::new().unwrap();
        let left = import_facts(self, definitions, &context);
        let right = import_facts(other, other_definitions, &context);
        let this = left.records();
        let other = right.records();
        // Fresh analysis must have different local identities. Compare slot facts
        // only after checking every key/receiver belongs to its actual lowering.
        fn normalized(table: &TypeTable, from: LoweringId, to: LoweringId) -> TypeTable {
            let mut result = table.clone();
            result.field_types = result
                .field_types
                .into_iter()
                .map(|(id, fact)| {
                    assert_eq!(id.arena(), from, "foreign field key");
                    (FieldId::new(to, id.owner(), id.slot()), fact)
                })
                .collect();
            macro_rules! keys {
                ($($field:ident : $ty:ident),+ $(,)?) => {$(
                    result.$field = result.$field.into_iter().map(|(id, fact)| {
                        assert_eq!(id.arena(), from, "foreign key in {}", stringify!($field));
                        (crate::hir::ids::$ty::new(to, id.owner(), id.index()), fact)
                    }).collect();
                )+};
            }

            keys!(propagations: ExprId, iterations: ExprId, protocol_receivers: ExprId, constraints: TypeRefId, type_refs: TypeRefId, expr_fields: ExprId,
                place_fields: PlaceId, place_indexes: PlaceId, place_index_writes: PlaceId, struct_inits: ExprId, enum_constructors: ExprId, exprs: ExprId, locals: LocalId,
                places: PlaceId, calls: ExprId, scalars: ExprId, pattern_scalars: PatternId, pattern_ranges: PatternId, pattern_fields: PatternId, pattern_variants: PatternId,
                callable_coercions: ExprId, interface_coercions: ExprId, associated_consts: ExprId);

            for call in result.calls.values_mut() {
                if let Some(receiver) = call.receiver {
                    assert_eq!(receiver.arena(), from);
                    call.receiver = Some(ExprId::new(to, receiver.owner(), receiver.index()));
                }
            }
            result
        }

        assert_eq!(
            normalized(this, arena, other_arena),
            normalized(other, other_arena, other_arena)
        );
    }
}

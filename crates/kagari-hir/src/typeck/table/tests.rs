//! Test-only comparison after validating and normalizing local node identities.

use crate::{
    hir::ids::{ExprId, FieldId, HirArenaId, PlaceId},
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
        arena: HirArenaId,
        other_arena: HirArenaId,
    ) {
        let context = DefinitionContext::new().unwrap();
        let left = import_facts(self, definitions, &context);
        let right = import_facts(other, other_definitions, &context);
        let this = left.records();
        let other = right.records();
        // Fresh analysis must have different local identities. Compare slot facts
        // only after checking every key/receiver belongs to its actual lowering.
        fn normalized(table: &TypeTable, from: HirArenaId, to: HirArenaId) -> TypeTable {
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

            keys!(propagations: ExprId, iterations: ExprId, protocol_receivers: ExprId, host_place_paths: PlaceId, host_paths: ExprId, constraints: TypeRefId, type_refs: TypeRefId, expr_fields: ExprId,
                place_fields: PlaceId, place_indexes: PlaceId, place_index_writes: PlaceId, struct_inits: ExprId, enum_constructors: ExprId, exprs: ExprId, locals: LocalId,
                places: PlaceId, calls: ExprId, scalars: ExprId, pattern_scalars: PatternId, pattern_ranges: PatternId, pattern_fields: PatternId, pattern_variants: PatternId,
                callable_coercions: ExprId, interface_coercions: ExprId, associated_consts: ExprId);
            for site in result.native_default_calls.values_mut() {
                assert_eq!(site.arena(), from);
                *site = ExprId::new(to, site.owner(), site.index());
            }
            for call in result.calls.values_mut() {
                if let Some(receiver) = call.receiver {
                    assert_eq!(receiver.arena(), from);
                    call.receiver = Some(ExprId::new(to, receiver.owner(), receiver.index()));
                }
            }
            for path in result.host_paths.values_mut() {
                assert_eq!(path.root.arena(), from);
                path.root = ExprId::new(to, path.root.owner(), path.root.index());
                for (_, argument) in &mut path.dynamic_arguments {
                    assert_eq!(argument.arena(), from);
                    *argument = ExprId::new(to, argument.owner(), argument.index());
                }
            }
            for path in result.host_place_paths.values_mut() {
                assert_eq!(path.root.arena(), from);
                path.root = PlaceId::new(to, path.root.owner(), path.root.index());
                for (_, argument) in &mut path.dynamic_arguments {
                    assert_eq!(argument.arena(), from);
                    *argument = ExprId::new(to, argument.owner(), argument.index());
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

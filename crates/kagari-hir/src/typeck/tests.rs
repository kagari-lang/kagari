//! Test-only comparisons of retained signature facts.

use crate::{
    hir::ids::{HirArenaId, ParamId},
    tests::common::import_facts,
    typeck::ModuleSignatures,
};
use kagari_common::identity::{map::DefinitionContext, table::DefinitionTable};

impl ModuleSignatures {
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
        let mut functions = this.functions.clone();
        for function in &mut functions {
            for param in &mut function.params {
                assert_eq!(param.id.arena(), arena);
                param.id = ParamId::new(other_arena, param.id.owner(), param.id.index());
            }
        }
        for function in &other.functions {
            for param in &function.params {
                assert_eq!(param.id.arena(), other_arena);
            }
        }
        assert_eq!(functions, other.functions);
        assert_eq!(this.type_bounds, other.type_bounds);
        assert_eq!(this.statics, other.statics);
        this.type_table.assert_same_source_facts(
            left.definitions(),
            &other.type_table,
            right.definitions(),
            arena,
            other_arena,
        );
    }
}

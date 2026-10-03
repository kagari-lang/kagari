use crate::{
    hir::ids::ExprId,
    language::semantics::ProtocolSemantics,
    typeck::{BodyTypeEnv, body::BodyChecker, table::ResolvedIteration},
    types::TypeId,
};
use kagari_abi::language::Protocol;
use kagari_common::identity::associated_type_id;

impl BodyChecker<'_> {
    pub(super) fn infer_iteration(
        &mut self,
        expr: ExprId,
        receiver: &TypeId,
        env: &BodyTypeEnv,
    ) -> Option<TypeId> {
        let (interface, iterator) =
            self.select_operator(receiver, Protocol::Iterable.nominal(), env)?;
        let member = associated_type_id(&interface.declaration, "Item");
        let item = interface
            .associated_types
            .get(&member)
            .cloned()
            .unwrap_or_else(|| {
                self.aggregates.normalize_type(&TypeId::Projection {
                    receiver: Box::new(receiver.clone()),
                    interface: Box::new(interface.clone()),
                    member,
                    arguments: vec![],
                })
            });
        let mut next = Protocol::Iterator.nominal();
        next.associated_types
            .insert(associated_type_id(&next.declaration, "Item"), item.clone());
        self.type_table.insert_iteration(
            expr,
            ResolvedIteration {
                into_interface: interface,
                iterator,
                next_interface: next,
                item: item.clone(),
            },
        );
        Some(item)
    }
}

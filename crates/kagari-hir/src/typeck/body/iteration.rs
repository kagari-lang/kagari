use super::*;
use crate::builtin::traits::StandardTrait;
use crate::types::{NominalType, associated_type_id};
impl BodyChecker<'_> {
    pub(super) fn add_iterator_view(&self, receiver: &TypeId, views: &mut Vec<NominalType>) {
        for kind in [StandardTrait::Iterator, StandardTrait::IntoIterator] {
            if matches!(
                receiver,
                TypeId::Cursor(_)
                    | TypeId::Array(_)
                    | TypeId::Map { .. }
                    | TypeId::Set(_)
                    | TypeId::Builtin(BuiltinType::String)
            ) && let Some(outputs) = crate::builtin::traits::iteration_outputs(
                kind,
                receiver,
                Some(self.aggregates),
                &Default::default(),
            ) && !views.iter().any(|n| n.declaration == kind.contract().id)
            {
                let mut view = kind.nominal();
                view.associated_types = outputs;
                views.push(view);
            }
        }
        if views
            .iter()
            .any(|n| n.declaration == StandardTrait::IntoIterator.contract().id)
        {
            return;
        }
        let Some(iterator) = views
            .iter()
            .find(|n| n.declaration == StandardTrait::Iterator.contract().id)
        else {
            return;
        };
        let member = associated_type_id(&iterator.declaration, "Item");
        let item = iterator
            .associated_types
            .get(&member)
            .cloned()
            .unwrap_or_else(|| {
                self.aggregates.normalize_type(&TypeId::Projection {
                    receiver: Box::new(receiver.clone()),
                    interface: Box::new(iterator.clone()),
                    member,
                    arguments: vec![],
                })
            });
        let mut into = StandardTrait::IntoIterator.nominal();
        into.associated_types
            .insert(associated_type_id(&into.declaration, "Item"), item);
        into.associated_types.insert(
            associated_type_id(&into.declaration, "IntoIter"),
            receiver.clone(),
        );
        views.push(into);
    }
    pub(super) fn infer_iteration(
        &mut self,
        expr: ExprId,
        receiver: &TypeId,
        env: &BodyTypeEnv,
    ) -> Option<TypeId> {
        let (interface, iterator) =
            self.select_operator(receiver, StandardTrait::IntoIterator.nominal(), env)?;
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
        let mut next = StandardTrait::Iterator.nominal();
        next.associated_types
            .insert(associated_type_id(&next.declaration, "Item"), item.clone());
        self.type_table.insert_iteration(
            expr,
            crate::typeck::ResolvedIteration {
                into_interface: interface,
                iterator,
                next_interface: next,
                item: item.clone(),
            },
        );
        Some(item)
    }
}

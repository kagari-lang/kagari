//! Actual graph access stays in scopes; the oracle never gains persistent roots.

use crate::{
    graph::model::{Id, Model, ROOTS, value},
    support::{Node, ObjectVec},
};
use kagari_gc::{
    access::Scope,
    dynamic::{DynamicLayoutBuilder, ReferenceField, ScalarField},
    error::GcError,
    heap::Heap,
    object::{AnyObject, Gc},
    root::{Root, RootSet},
};
use std::collections::{BTreeMap, VecDeque};

pub(super) struct Storage {
    pub roots: RootSet,
    pub vector: Root<ObjectVec>,
    pub dynamic: Root<AnyObject>,
    pub links: [ReferenceField; 2],
    pub label: ScalarField<u64>,
}

impl Storage {
    pub fn new(heap: &mut Heap) -> Result<Self, GcError> {
        let roots = heap.access()?.create_roots(ROOTS)?;
        let vector = ObjectVec::alloc(&mut heap.access()?, &[])?;
        let mut builder = DynamicLayoutBuilder::new();
        let label = builder.scalar::<u64>();
        let left = builder.reference();
        let right = builder.reference();
        let layout = heap.access()?.register_dynamic(builder)?;
        let dynamic = heap.scope(|cx| {
            let object = cx.alloc_dynamic(&layout)?;
            cx.retain(object)
        })?;
        Ok(Self {
            roots,
            vector,
            dynamic,
            links: [layout.reference(left)?, layout.reference(right)?],
            label: layout.scalar(label)?,
        })
    }

    /// Verify the complete observable graph, returning handles only within 'gc.
    pub fn handles<'gc>(
        &self,
        cx: &Scope<'gc>,
        model: &Model,
    ) -> Result<BTreeMap<Id, Gc<'gc, Node>>, GcError> {
        let mut queue = VecDeque::new();
        for (slot, expected) in model.roots.iter().enumerate() {
            match expected {
                Some(id) => queue.push_back((*id, cx.get_root(&self.roots, slot)?)),
                None => assert!(matches!(
                    cx.get_root::<Node>(&self.roots, slot),
                    Err(GcError::Index)
                )),
            }
        }
        let vector = cx.get(&self.vector)?;
        for (index, &id) in model.vector.iter().enumerate() {
            queue.push_back((
                id,
                cx.follow(vector, |v| v.item_edge(index))
                    .expect("model vector element"),
            ));
        }
        assert!(
            cx.follow(vector, |v| v.item_edge(model.vector.len()))
                .is_none(),
            "extra vector element"
        );
        let dynamic = cx.get(&self.dynamic)?;
        assert_eq!(self.label.get(cx, dynamic)?, model.label);
        for (field, expected) in self.links.iter().zip(model.dynamic) {
            match (field.get(cx, dynamic)?, expected) {
                (Some(object), Some(id)) => queue.push_back((id, cx.downcast::<Node>(object)?)),
                (None, None) => {}
                _ => panic!("dynamic edge disagrees with model"),
            }
        }
        let mut found = BTreeMap::new();
        while let Some((id, object)) = queue.pop_front() {
            assert_eq!(
                Node::VALUE.get(cx, object),
                value(id, model.nodes[id].tag),
                "object {id}"
            );
            if found.insert(id, object).is_some() {
                continue;
            }
            match (Node::NEXT.get(cx, object), model.nodes[id].next) {
                (Some(next), Some(id)) => queue.push_back((id, next)),
                (None, None) => {}
                _ => panic!("node {id} edge disagrees with model"),
            }
        }
        assert_eq!(
            found.len(),
            model.reachable().into_iter().filter(|v| *v).count()
        );
        Ok(found)
    }
}

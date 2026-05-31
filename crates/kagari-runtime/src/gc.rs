use std::{cell::RefCell, collections::HashSet};

use crate::value::{StructValueField, Value};

#[derive(Debug, Clone, Copy)]
pub struct GcHeapConfig {
    pub nursery_bytes: usize,
    pub large_object_threshold: usize,
}

impl Default for GcHeapConfig {
    fn default() -> Self {
        Self {
            nursery_bytes: 1024 * 1024,
            large_object_threshold: 8 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeapObjectId(u64);

impl HeapObjectId {
    pub fn new(index: usize) -> Self {
        Self(index as u64)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GcRootId(u64);

impl GcRootId {
    pub fn new(index: usize) -> Self {
        Self(index as u64)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcObjectKind {
    Array,
    Struct,
}

#[derive(Debug, Clone)]
enum HeapObject {
    Array(Vec<Value>),
    Struct {
        name: String,
        fields: Vec<StructValueField>,
    },
}

#[derive(Debug)]
pub struct GcHeap {
    config: GcHeapConfig,
    objects: RefCell<Vec<HeapObject>>,
    roots: RefCell<Vec<Option<Value>>>,
}

impl GcHeap {
    pub fn new(config: GcHeapConfig) -> Self {
        Self {
            config,
            objects: RefCell::new(Vec::new()),
            roots: RefCell::new(Vec::new()),
        }
    }

    pub fn config(&self) -> GcHeapConfig {
        self.config
    }

    pub fn allocated_objects(&self) -> usize {
        self.objects.borrow().len()
    }

    pub fn active_roots(&self) -> usize {
        self.roots.borrow().iter().flatten().count()
    }

    pub fn alloc_array(&self, elements: Vec<Value>) -> Option<HeapObjectId> {
        if !elements.iter().all(Value::is_default_heap_payload) {
            return None;
        }
        Some(self.alloc_object(HeapObject::Array(elements)))
    }

    pub fn alloc_struct(
        &self,
        name: String,
        fields: Vec<StructValueField>,
    ) -> Option<HeapObjectId> {
        if !fields
            .iter()
            .all(|field| field.value.is_default_heap_payload())
        {
            return None;
        }
        Some(self.alloc_object(HeapObject::Struct { name, fields }))
    }

    pub fn array_len(&self, id: HeapObjectId) -> Option<usize> {
        self.with_array(id, |elements| elements.len())
    }

    pub fn array_snapshot(&self, id: HeapObjectId) -> Option<Vec<Value>> {
        self.with_array(id, |elements| elements.clone())
    }

    pub fn array_get(&self, id: HeapObjectId, index: usize) -> Option<Value> {
        self.with_array(id, |elements| elements.get(index).cloned())
            .flatten()
    }

    pub fn array_push(&self, id: HeapObjectId, value: Value) -> Option<()> {
        if !value.is_default_heap_payload() {
            return None;
        }
        self.with_array_mut(id, |elements| {
            elements.push(value);
        })
    }

    pub fn array_pop(&self, id: HeapObjectId) -> Option<Value> {
        self.with_array_mut(id, |elements| elements.pop()).flatten()
    }

    pub fn array_set(&self, id: HeapObjectId, index: usize, value: Value) -> Option<()> {
        if !value.is_default_heap_payload() {
            return None;
        }
        self.with_array_mut(id, |elements| {
            let slot = elements.get_mut(index)?;
            *slot = value;
            Some(())
        })
        .flatten()
    }

    pub fn struct_name(&self, id: HeapObjectId) -> Option<String> {
        self.with_struct(id, |name, _| name.clone())
    }

    pub fn struct_snapshot(&self, id: HeapObjectId) -> Option<(String, Vec<StructValueField>)> {
        self.with_struct(id, |name, fields| (name.clone(), fields.clone()))
    }

    pub fn struct_get_field(&self, id: HeapObjectId, field_name: &str) -> Option<Value> {
        self.with_struct(id, |_, fields| {
            fields
                .iter()
                .find(|field| field.name == field_name)
                .map(|field| field.value.clone())
        })
        .flatten()
    }

    pub fn struct_set_field(
        &self,
        id: HeapObjectId,
        field_name: &str,
        next_value: Value,
    ) -> Option<()> {
        if !next_value.is_default_heap_payload() {
            return None;
        }
        self.with_struct_mut(id, |_, fields| {
            let field = fields.iter_mut().find(|field| field.name == field_name)?;
            field.value = next_value;
            Some(())
        })
        .flatten()
    }

    pub fn object_kind(&self, id: HeapObjectId) -> Option<GcObjectKind> {
        let objects = self.objects.borrow();
        match objects.get(id.index())? {
            HeapObject::Array(_) => Some(GcObjectKind::Array),
            HeapObject::Struct { .. } => Some(GcObjectKind::Struct),
        }
    }

    pub fn root_value(&self, value: Value) -> Option<GcRootId> {
        if !value.is_storable() {
            return None;
        }

        let mut roots = self.roots.borrow_mut();
        let id = GcRootId::new(roots.len());
        roots.push(Some(value));
        Some(id)
    }

    pub fn root_snapshot(&self, id: GcRootId) -> Option<Value> {
        self.roots.borrow().get(id.index())?.clone()
    }

    pub fn update_root(&self, id: GcRootId, value: Value) -> Option<()> {
        if !value.is_storable() {
            return None;
        }

        let mut roots = self.roots.borrow_mut();
        let slot = roots.get_mut(id.index())?;
        slot.as_ref()?;
        *slot = Some(value);
        Some(())
    }

    pub fn release_root(&self, id: GcRootId) -> Option<Value> {
        self.roots.borrow_mut().get_mut(id.index())?.take()
    }

    pub fn trace_roots(&self) -> Vec<HeapObjectId> {
        let roots = self
            .roots
            .borrow()
            .iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        self.trace_values(&roots)
    }

    pub fn trace_value(&self, value: &Value) -> Vec<HeapObjectId> {
        self.trace_values(std::slice::from_ref(value))
    }

    fn alloc_object(&self, object: HeapObject) -> HeapObjectId {
        let mut objects = self.objects.borrow_mut();
        let id = HeapObjectId::new(objects.len());
        objects.push(object);
        id
    }

    fn trace_values(&self, values: &[Value]) -> Vec<HeapObjectId> {
        let mut seen = HashSet::new();
        let mut traced = Vec::new();
        for value in values {
            self.trace_value_inner(value, &mut seen, &mut traced);
        }
        traced
    }

    fn trace_value_inner(
        &self,
        value: &Value,
        seen: &mut HashSet<HeapObjectId>,
        traced: &mut Vec<HeapObjectId>,
    ) {
        match value {
            Value::Tuple(elements) => {
                for element in elements {
                    self.trace_value_inner(element, seen, traced);
                }
            }
            Value::Array(id) | Value::Struct(id) | Value::GcHandle(id) => {
                self.trace_object(*id, seen, traced);
            }
            Value::Unit
            | Value::Bool(_)
            | Value::I32(_)
            | Value::I64(_)
            | Value::F32(_)
            | Value::F64(_)
            | Value::Str(_)
            | Value::Interface(_)
            | Value::HostOwned(_)
            | Value::HostPathView(_)
            | Value::Ephemeral(_) => {}
        }
    }

    fn trace_object(
        &self,
        id: HeapObjectId,
        seen: &mut HashSet<HeapObjectId>,
        traced: &mut Vec<HeapObjectId>,
    ) {
        let Some(object) = self.objects.borrow().get(id.index()).cloned() else {
            return;
        };
        if !seen.insert(id) {
            return;
        }
        traced.push(id);

        match object {
            HeapObject::Array(elements) => {
                for element in elements {
                    self.trace_value_inner(&element, seen, traced);
                }
            }
            HeapObject::Struct { fields, .. } => {
                for field in fields {
                    self.trace_value_inner(&field.value, seen, traced);
                }
            }
        }
    }

    fn with_array<R>(&self, id: HeapObjectId, f: impl FnOnce(&Vec<Value>) -> R) -> Option<R> {
        let objects = self.objects.borrow();
        match objects.get(id.index())? {
            HeapObject::Array(elements) => Some(f(elements)),
            HeapObject::Struct { .. } => None,
        }
    }

    fn with_array_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&mut Vec<Value>) -> R,
    ) -> Option<R> {
        let mut objects = self.objects.borrow_mut();
        match objects.get_mut(id.index())? {
            HeapObject::Array(elements) => Some(f(elements)),
            HeapObject::Struct { .. } => None,
        }
    }

    fn with_struct<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&String, &Vec<StructValueField>) -> R,
    ) -> Option<R> {
        let objects = self.objects.borrow();
        match objects.get(id.index())? {
            HeapObject::Struct { name, fields } => Some(f(name, fields)),
            HeapObject::Array(_) => None,
        }
    }

    fn with_struct_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&mut String, &mut Vec<StructValueField>) -> R,
    ) -> Option<R> {
        let mut objects = self.objects.borrow_mut();
        match objects.get_mut(id.index())? {
            HeapObject::Struct { name, fields } => Some(f(name, fields)),
            HeapObject::Array(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        host::HostObjectId,
        value::{HostPathViewId, StructValueField},
    };

    #[test]
    fn rejects_ephemeral_values_as_heap_payloads() {
        let heap = GcHeap::new(GcHeapConfig::default());

        assert!(
            heap.alloc_array(vec![Value::host_ref(HostObjectId(1))])
                .is_none()
        );
        assert!(
            heap.alloc_array(vec![Value::host_mut(HostObjectId(2))])
                .is_none()
        );
        assert_eq!(heap.allocated_objects(), 0);
    }

    #[test]
    fn rejects_host_handles_and_path_views_as_default_heap_payloads() {
        let heap = GcHeap::new(GcHeapConfig::default());

        assert!(
            heap.alloc_array(vec![Value::HostOwned(HostObjectId(1))])
                .is_none()
        );
        assert!(
            heap.alloc_struct(
                "HostBacked".to_owned(),
                vec![StructValueField {
                    name: "path".to_owned(),
                    value: Value::HostPathView(HostPathViewId(3)),
                }],
            )
            .is_none()
        );
        assert_eq!(heap.allocated_objects(), 0);
    }

    #[test]
    fn rejects_non_storable_heap_mutations() {
        let heap = GcHeap::new(GcHeapConfig::default());
        let array = heap.alloc_array(vec![Value::I32(1)]).unwrap();
        let record = heap
            .alloc_struct(
                "Record".to_owned(),
                vec![StructValueField {
                    name: "value".to_owned(),
                    value: Value::I32(1),
                }],
            )
            .unwrap();

        assert!(
            heap.array_push(array, Value::host_ref(HostObjectId(1)))
                .is_none()
        );
        assert!(
            heap.array_set(array, 0, Value::HostPathView(HostPathViewId(4)))
                .is_none()
        );
        assert!(
            heap.struct_set_field(record, "value", Value::HostOwned(HostObjectId(5)))
                .is_none()
        );

        assert_eq!(heap.array_snapshot(array), Some(vec![Value::I32(1)]));
        assert_eq!(heap.struct_get_field(record, "value"), Some(Value::I32(1)));
    }

    #[test]
    fn assigns_stable_object_identity_and_kind() {
        let heap = GcHeap::new(GcHeapConfig::default());
        let first = heap.alloc_array(vec![]).unwrap();
        let second = heap
            .alloc_struct(
                "Empty".to_owned(),
                vec![StructValueField {
                    name: "value".to_owned(),
                    value: Value::Unit,
                }],
            )
            .unwrap();

        assert_ne!(first, second);
        assert_eq!(first.index(), 0);
        assert_eq!(second.index(), 1);
        assert_eq!(heap.object_kind(first), Some(GcObjectKind::Array));
        assert_eq!(heap.object_kind(second), Some(GcObjectKind::Struct));
    }

    #[test]
    fn roots_are_explicit_storable_slots() {
        let heap = GcHeap::new(GcHeapConfig::default());
        let root = heap.root_value(Value::HostOwned(HostObjectId(1))).unwrap();

        assert_eq!(root.index(), 0);
        assert_eq!(
            heap.root_snapshot(root),
            Some(Value::HostOwned(HostObjectId(1)))
        );
        assert_eq!(heap.active_roots(), 1);
        assert_eq!(heap.trace_roots(), Vec::<HeapObjectId>::new());

        assert!(
            heap.root_value(Value::Tuple(vec![Value::host_ref(HostObjectId(2))]))
                .is_none()
        );
        assert_eq!(heap.active_roots(), 1);
    }

    #[test]
    fn root_scanning_traces_only_gc_managed_boundaries() {
        let heap = GcHeap::new(GcHeapConfig::default());
        let leaf = heap.alloc_array(vec![Value::I32(1)]).unwrap();
        let record = heap
            .alloc_struct(
                "Record".to_owned(),
                vec![StructValueField {
                    name: "leaf".to_owned(),
                    value: Value::Array(leaf),
                }],
            )
            .unwrap();

        let root = heap
            .root_value(Value::Tuple(vec![
                Value::Struct(record),
                Value::HostOwned(HostObjectId(7)),
                Value::HostPathView(HostPathViewId(8)),
            ]))
            .unwrap();

        assert_eq!(heap.trace_roots(), vec![record, leaf]);

        heap.update_root(root, Value::GcHandle(leaf)).unwrap();
        assert_eq!(heap.trace_roots(), vec![leaf]);

        assert_eq!(heap.release_root(root), Some(Value::GcHandle(leaf)));
        assert_eq!(heap.trace_roots(), Vec::<HeapObjectId>::new());
    }

    #[test]
    fn root_scanning_handles_cycles_without_duplicate_identity() {
        let heap = GcHeap::new(GcHeapConfig::default());
        let array = heap.alloc_array(vec![]).unwrap();
        let record = heap
            .alloc_struct(
                "Cycle".to_owned(),
                vec![StructValueField {
                    name: "array".to_owned(),
                    value: Value::Array(array),
                }],
            )
            .unwrap();
        heap.array_push(array, Value::Struct(record)).unwrap();
        heap.root_value(Value::Array(array)).unwrap();

        assert_eq!(heap.trace_roots(), vec![array, record]);
    }
}

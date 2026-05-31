use std::cell::RefCell;

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
}

impl GcHeap {
    pub fn new(config: GcHeapConfig) -> Self {
        Self {
            config,
            objects: RefCell::new(Vec::new()),
        }
    }

    pub fn config(&self) -> GcHeapConfig {
        self.config
    }

    pub fn allocated_objects(&self) -> usize {
        self.objects.borrow().len()
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

    fn alloc_object(&self, object: HeapObject) -> HeapObjectId {
        let mut objects = self.objects.borrow_mut();
        let id = HeapObjectId::new(objects.len());
        objects.push(object);
        id
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
}

use std::{cell::RefCell, collections::HashSet};

use indexmap::{IndexMap, IndexSet};

use crate::value::{EnumValueSnapshot, MapKey, StructValueField, Value};

#[derive(Debug, Clone, Copy)]
pub struct GcHeapConfig {
    pub nursery_bytes: usize,
    pub large_object_threshold: usize,
    pub max_heap_units: Option<usize>,
}

impl Default for GcHeapConfig {
    fn default() -> Self {
        Self {
            nursery_bytes: 1024 * 1024,
            large_object_threshold: 8 * 1024,
            max_heap_units: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GcHeapStats {
    pub current_heap_units: usize,
    pub peak_heap_units: usize,
    pub allocated_objects: usize,
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
    Map,
    Set,
    Enum,
    Struct,
}

#[derive(Debug, Clone)]
enum HeapObject {
    Array(Vec<Value>),
    Map(IndexMap<MapKey, Value>),
    Set(IndexSet<MapKey>),
    Enum(EnumValueSnapshot),
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
    stats: RefCell<GcHeapStats>,
}

impl GcHeap {
    pub fn new(config: GcHeapConfig) -> Self {
        Self {
            config,
            objects: RefCell::new(Vec::new()),
            roots: RefCell::new(Vec::new()),
            stats: RefCell::new(GcHeapStats::default()),
        }
    }

    pub fn config(&self) -> GcHeapConfig {
        self.config
    }

    pub fn allocated_objects(&self) -> usize {
        self.objects.borrow().len()
    }

    pub fn stats(&self) -> GcHeapStats {
        let mut stats = *self.stats.borrow();
        stats.allocated_objects = self.allocated_objects();
        stats
    }

    pub fn active_roots(&self) -> usize {
        self.roots.borrow().iter().flatten().count()
    }

    pub fn alloc_array(&self, elements: Vec<Value>) -> Option<HeapObjectId> {
        if !elements.iter().all(Value::is_default_heap_payload) {
            return None;
        }
        self.reserve_heap_units(1 + elements.len())?;
        Some(self.alloc_object(HeapObject::Array(elements)))
    }

    pub fn alloc_map(&self, entries: Vec<(Value, Value)>) -> Option<HeapObjectId> {
        let mut map = IndexMap::new();
        for (key, value) in entries {
            if !value.is_default_heap_payload() {
                return None;
            }
            let key = MapKey::from_value(&key)?;
            map.insert(key, value);
        }
        self.reserve_heap_units(1 + map.len())?;
        Some(self.alloc_object(HeapObject::Map(map)))
    }

    pub fn alloc_set(&self, values: Vec<Value>) -> Option<HeapObjectId> {
        let mut set = IndexSet::new();
        for value in values {
            let key = MapKey::from_value(&value)?;
            set.insert(key);
        }
        self.reserve_heap_units(1 + set.len())?;
        Some(self.alloc_object(HeapObject::Set(set)))
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
        self.reserve_heap_units(1 + fields.len())?;
        Some(self.alloc_object(HeapObject::Struct { name, fields }))
    }

    pub fn alloc_enum(
        &self,
        name: String,
        variant: String,
        fields: Vec<Value>,
    ) -> Option<HeapObjectId> {
        if !fields.iter().all(Value::is_default_heap_payload) {
            return None;
        }
        self.reserve_heap_units(1 + fields.len())?;
        Some(self.alloc_object(HeapObject::Enum(EnumValueSnapshot {
            name,
            variant,
            fields,
        })))
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
        self.reserve_heap_units(1)?;
        self.with_array_mut(id, |elements| {
            elements.push(value);
        })
    }

    pub fn array_pop(&self, id: HeapObjectId) -> Option<Value> {
        let value = self.with_array_mut(id, |elements| elements.pop()).flatten();
        if value.is_some() {
            self.release_heap_units(1);
        }
        value
    }

    pub fn array_insert(&self, id: HeapObjectId, index: usize, value: Value) -> Option<()> {
        if !value.is_default_heap_payload() {
            return None;
        }
        let valid_index = self.with_array(id, |elements| index <= elements.len())?;
        if !valid_index {
            return None;
        }
        self.reserve_heap_units(1)?;
        let inserted = self.with_array_mut(id, |elements| {
            elements.insert(index, value);
        });
        if inserted.is_none() {
            self.release_heap_units(1);
        }
        inserted
    }

    pub fn array_remove(&self, id: HeapObjectId, index: usize) -> Option<Value> {
        let value = self
            .with_array_mut(id, |elements| {
                (index < elements.len()).then(|| elements.remove(index))
            })
            .flatten();
        if value.is_some() {
            self.release_heap_units(1);
        }
        value
    }

    pub fn array_clear(&self, id: HeapObjectId) -> Option<()> {
        let removed = self.with_array_mut(id, |elements| {
            let removed = elements.len();
            elements.clear();
            removed
        })?;
        self.release_heap_units(removed);
        Some(())
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

    pub fn map_len(&self, id: HeapObjectId) -> Option<usize> {
        self.with_map(id, |entries| entries.len())
    }

    pub fn map_snapshot(&self, id: HeapObjectId) -> Option<Vec<(Value, Value)>> {
        self.with_map(id, |entries| {
            entries
                .iter()
                .map(|(key, value)| (key.to_value(), value.clone()))
                .collect()
        })
    }

    pub fn map_get(&self, id: HeapObjectId, key: &Value) -> Option<Value> {
        let key = MapKey::from_value(key)?;
        self.with_map(id, |entries| entries.get(&key).cloned())
            .flatten()
    }

    pub fn map_insert(&self, id: HeapObjectId, key: Value, value: Value) -> Option<()> {
        if !value.is_default_heap_payload() {
            return None;
        }
        let key = MapKey::from_value(&key)?;
        let needs_unit = self.with_map(id, |entries| !entries.contains_key(&key))?;
        if needs_unit {
            self.reserve_heap_units(1)?;
        }
        let inserted = self.with_map_mut(id, |entries| {
            entries.insert(key, value);
        });
        if inserted.is_none() && needs_unit {
            self.release_heap_units(1);
        }
        inserted
    }

    pub fn map_remove(&self, id: HeapObjectId, key: &Value) -> Option<Value> {
        let key = MapKey::from_value(key)?;
        let value = self
            .with_map_mut(id, |entries| entries.shift_remove(&key))
            .flatten();
        if value.is_some() {
            self.release_heap_units(1);
        }
        value
    }

    pub fn map_clear(&self, id: HeapObjectId) -> Option<()> {
        let removed = self.with_map_mut(id, |entries| {
            let removed = entries.len();
            entries.clear();
            removed
        })?;
        self.release_heap_units(removed);
        Some(())
    }

    pub fn set_len(&self, id: HeapObjectId) -> Option<usize> {
        self.with_set(id, |values| values.len())
    }

    pub fn set_snapshot(&self, id: HeapObjectId) -> Option<Vec<Value>> {
        self.with_set(id, |values| values.iter().map(MapKey::to_value).collect())
    }

    pub fn set_contains(&self, id: HeapObjectId, value: &Value) -> Option<bool> {
        let key = MapKey::from_value(value)?;
        self.with_set(id, |values| values.contains(&key))
    }

    pub fn set_insert(&self, id: HeapObjectId, value: Value) -> Option<bool> {
        let key = MapKey::from_value(&value)?;
        let needs_unit = self.with_set(id, |values| !values.contains(&key))?;
        if needs_unit {
            self.reserve_heap_units(1)?;
        }
        let inserted = self.with_set_mut(id, |values| values.insert(key));
        if inserted.is_none() && needs_unit {
            self.release_heap_units(1);
        }
        inserted
    }

    pub fn set_remove(&self, id: HeapObjectId, value: &Value) -> Option<bool> {
        let key = MapKey::from_value(value)?;
        let removed = self.with_set_mut(id, |values| values.shift_remove(&key))?;
        if removed {
            self.release_heap_units(1);
        }
        Some(removed)
    }

    pub fn set_clear(&self, id: HeapObjectId) -> Option<()> {
        let removed = self.with_set_mut(id, |values| {
            let removed = values.len();
            values.clear();
            removed
        })?;
        self.release_heap_units(removed);
        Some(())
    }

    pub fn struct_name(&self, id: HeapObjectId) -> Option<String> {
        self.with_struct(id, |name, _| name.clone())
    }

    pub fn enum_snapshot(&self, id: HeapObjectId) -> Option<EnumValueSnapshot> {
        self.with_enum(id, Clone::clone)
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
            HeapObject::Map(_) => Some(GcObjectKind::Map),
            HeapObject::Set(_) => Some(GcObjectKind::Set),
            HeapObject::Enum(_) => Some(GcObjectKind::Enum),
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

    fn reserve_heap_units(&self, units: usize) -> Option<()> {
        let mut stats = self.stats.borrow_mut();
        let next = stats.current_heap_units.checked_add(units)?;
        if let Some(max) = self.config.max_heap_units {
            if next > max {
                return None;
            }
        }
        stats.current_heap_units = next;
        stats.peak_heap_units = stats.peak_heap_units.max(next);
        Some(())
    }

    fn release_heap_units(&self, units: usize) {
        let mut stats = self.stats.borrow_mut();
        stats.current_heap_units = stats.current_heap_units.saturating_sub(units);
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
            Value::Array(id)
            | Value::Map(id)
            | Value::Set(id)
            | Value::Enum(id)
            | Value::Struct(id)
            | Value::GcHandle(id) => {
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
            | Value::HostRoot(_)
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
            HeapObject::Map(entries) => {
                for value in entries.values() {
                    self.trace_value_inner(value, seen, traced);
                }
            }
            HeapObject::Set(_) => {}
            HeapObject::Enum(snapshot) => {
                for field in snapshot.fields {
                    self.trace_value_inner(&field, seen, traced);
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
            HeapObject::Map(_) | HeapObject::Set(_) | HeapObject::Enum(_) => None,
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
            HeapObject::Map(_) | HeapObject::Set(_) | HeapObject::Enum(_) => None,
            HeapObject::Struct { .. } => None,
        }
    }

    fn with_map<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&IndexMap<MapKey, Value>) -> R,
    ) -> Option<R> {
        let objects = self.objects.borrow();
        match objects.get(id.index())? {
            HeapObject::Map(entries) => Some(f(entries)),
            HeapObject::Array(_)
            | HeapObject::Set(_)
            | HeapObject::Enum(_)
            | HeapObject::Struct { .. } => None,
        }
    }

    fn with_map_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&mut IndexMap<MapKey, Value>) -> R,
    ) -> Option<R> {
        let mut objects = self.objects.borrow_mut();
        match objects.get_mut(id.index())? {
            HeapObject::Map(entries) => Some(f(entries)),
            HeapObject::Array(_)
            | HeapObject::Set(_)
            | HeapObject::Enum(_)
            | HeapObject::Struct { .. } => None,
        }
    }

    fn with_set<R>(&self, id: HeapObjectId, f: impl FnOnce(&IndexSet<MapKey>) -> R) -> Option<R> {
        let objects = self.objects.borrow();
        match objects.get(id.index())? {
            HeapObject::Set(values) => Some(f(values)),
            HeapObject::Array(_)
            | HeapObject::Map(_)
            | HeapObject::Enum(_)
            | HeapObject::Struct { .. } => None,
        }
    }

    fn with_set_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&mut IndexSet<MapKey>) -> R,
    ) -> Option<R> {
        let mut objects = self.objects.borrow_mut();
        match objects.get_mut(id.index())? {
            HeapObject::Set(values) => Some(f(values)),
            HeapObject::Array(_)
            | HeapObject::Map(_)
            | HeapObject::Enum(_)
            | HeapObject::Struct { .. } => None,
        }
    }

    fn with_enum<R>(&self, id: HeapObjectId, f: impl FnOnce(&EnumValueSnapshot) -> R) -> Option<R> {
        let objects = self.objects.borrow();
        match objects.get(id.index())? {
            HeapObject::Enum(snapshot) => Some(f(snapshot)),
            HeapObject::Array(_)
            | HeapObject::Map(_)
            | HeapObject::Set(_)
            | HeapObject::Struct { .. } => None,
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
            HeapObject::Array(_)
            | HeapObject::Map(_)
            | HeapObject::Set(_)
            | HeapObject::Enum(_) => None,
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
            HeapObject::Array(_)
            | HeapObject::Map(_)
            | HeapObject::Set(_)
            | HeapObject::Enum(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        host::{
            DynamicPathArguments, HostBorrowTable, HostObjectId, HostPathDescriptorRegistration,
            HostPathSegment, HostRegistry, HostRootHandle, HostSchemaEpoch, HostTypeInfo,
            HostTypeOwnership,
        },
        metadata::{AbiFingerprint, FieldMetadataId, PathAccess, TypeId},
        value::StructValueField,
    };

    fn host_root_value(object_id: u64) -> Value {
        Value::HostRoot(HostRootHandle::new(
            HostObjectId(object_id),
            TypeId::new(0),
            HostSchemaEpoch::new(0),
            AbiFingerprint(1),
        ))
    }

    fn path_view_value(object_id: u64) -> Value {
        let root_type = TypeId::new(0);
        let result_type = TypeId::new(1);
        let mut registry = HostRegistry::default();
        registry
            .register_type(HostTypeInfo {
                type_id: root_type,
                script_name: "Player".to_owned(),
                rust_type_name: "Player".to_owned(),
                ownership: HostTypeOwnership::HostRoot,
                fields: Vec::new(),
                methods: Vec::new(),
                traits: Vec::new(),
                path_access: PathAccess::ReadWrite,
                reflection: crate::host::HostReflectionPolicy::Hidden,
                abi_fingerprint: AbiFingerprint(1),
            })
            .unwrap();
        let root = registry
            .register_root(HostObjectId(object_id), root_type, HostSchemaEpoch::new(0))
            .unwrap();
        let descriptor = registry
            .register_path_descriptor(HostPathDescriptorRegistration {
                root_type,
                result_type,
                segments: vec![HostPathSegment::Field {
                    name: "hp".to_owned(),
                    field_id: FieldMetadataId::new(0),
                    owner_type: root_type,
                    result_type,
                    access: PathAccess::ReadWrite,
                    abi_fingerprint: AbiFingerprint(2),
                }],
                access: PathAccess::ReadWrite,
                schema_epoch: HostSchemaEpoch::new(0),
                abi_fingerprint: AbiFingerprint(3),
                capability_requirements: crate::security::CapabilitySet::default(),
            })
            .unwrap();
        Value::HostPathView(
            registry
                .make_path_view(root, descriptor, DynamicPathArguments::empty())
                .unwrap(),
        )
    }

    fn shared_borrow_value(object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame();
        Value::host_ref(
            guard
                .borrow_shared(HostObjectId(object_id), TypeId::new(0))
                .unwrap(),
        )
    }

    fn unique_borrow_value(object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame();
        Value::host_mut(
            guard
                .borrow_unique(HostObjectId(object_id), TypeId::new(0))
                .unwrap(),
        )
    }

    #[test]
    fn rejects_ephemeral_values_as_heap_payloads() {
        let heap = GcHeap::new(GcHeapConfig::default());

        assert!(heap.alloc_array(vec![shared_borrow_value(1)]).is_none());
        assert!(heap.alloc_array(vec![unique_borrow_value(2)]).is_none());
        assert_eq!(heap.allocated_objects(), 0);
    }

    #[test]
    fn rejects_host_handles_and_path_views_as_default_heap_payloads() {
        let heap = GcHeap::new(GcHeapConfig::default());

        assert!(heap.alloc_array(vec![host_root_value(1)]).is_none());
        assert!(
            heap.alloc_map(vec![(Value::Str("host".to_owned()), host_root_value(2))])
                .is_none()
        );
        assert!(
            heap.alloc_map(vec![(path_view_value(3), Value::I32(1))])
                .is_none()
        );
        assert!(heap.alloc_set(vec![host_root_value(4)]).is_none());
        assert!(
            heap.alloc_struct(
                "HostBacked".to_owned(),
                vec![StructValueField {
                    name: "path".to_owned(),
                    value: path_view_value(3),
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

        assert!(heap.array_push(array, shared_borrow_value(1)).is_none());
        assert!(heap.array_set(array, 0, path_view_value(4)).is_none());
        assert!(
            heap.struct_set_field(record, "value", host_root_value(5))
                .is_none()
        );

        assert_eq!(heap.array_snapshot(array), Some(vec![Value::I32(1)]));
        assert_eq!(heap.struct_get_field(record, "value"), Some(Value::I32(1)));
    }

    #[test]
    fn assigns_stable_object_identity_and_kind() {
        let heap = GcHeap::new(GcHeapConfig::default());
        let first = heap.alloc_array(vec![]).unwrap();
        let second = heap.alloc_map(vec![]).unwrap();
        let third = heap.alloc_set(vec![]).unwrap();
        let fourth = heap
            .alloc_struct(
                "Empty".to_owned(),
                vec![StructValueField {
                    name: "value".to_owned(),
                    value: Value::Unit,
                }],
            )
            .unwrap();

        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(third, fourth);
        assert_eq!(first.index(), 0);
        assert_eq!(second.index(), 1);
        assert_eq!(third.index(), 2);
        assert_eq!(fourth.index(), 3);
        assert_eq!(heap.object_kind(first), Some(GcObjectKind::Array));
        assert_eq!(heap.object_kind(second), Some(GcObjectKind::Map));
        assert_eq!(heap.object_kind(third), Some(GcObjectKind::Set));
        assert_eq!(heap.object_kind(fourth), Some(GcObjectKind::Struct));
    }

    #[test]
    fn builtin_ordered_maps_preserve_insertion_order_and_account_units() {
        let heap = GcHeap::new(GcHeapConfig::default());
        let map = heap
            .alloc_map(vec![
                (Value::Str("b".to_owned()), Value::I32(2)),
                (Value::Str("a".to_owned()), Value::I32(1)),
                (Value::Str("b".to_owned()), Value::I32(3)),
            ])
            .unwrap();

        assert_eq!(heap.map_len(map), Some(2));
        assert_eq!(
            heap.map_snapshot(map),
            Some(vec![
                (Value::Str("b".to_owned()), Value::I32(3)),
                (Value::Str("a".to_owned()), Value::I32(1)),
            ])
        );
        assert_eq!(heap.stats().current_heap_units, 3);

        heap.map_insert(map, Value::Str("c".to_owned()), Value::I64(4))
            .unwrap();
        assert_eq!(heap.stats().current_heap_units, 4);
        assert_eq!(
            heap.map_get(map, &Value::Str("c".to_owned())),
            Some(Value::I64(4))
        );

        heap.map_insert(map, Value::Str("a".to_owned()), Value::I32(9))
            .unwrap();
        assert_eq!(heap.stats().current_heap_units, 4);
        assert_eq!(
            heap.map_snapshot(map).unwrap(),
            vec![
                (Value::Str("b".to_owned()), Value::I32(3)),
                (Value::Str("a".to_owned()), Value::I32(9)),
                (Value::Str("c".to_owned()), Value::I64(4)),
            ]
        );

        assert_eq!(
            heap.map_remove(map, &Value::Str("b".to_owned())),
            Some(Value::I32(3))
        );
        assert_eq!(heap.stats().current_heap_units, 3);
        heap.map_clear(map).unwrap();
        assert_eq!(heap.map_snapshot(map), Some(vec![]));
        assert_eq!(heap.stats().current_heap_units, 1);
    }

    #[test]
    fn builtin_ordered_sets_preserve_insertion_order_and_account_units() {
        let heap = GcHeap::new(GcHeapConfig::default());
        let set = heap
            .alloc_set(vec![
                Value::Str("b".to_owned()),
                Value::Str("a".to_owned()),
                Value::Str("b".to_owned()),
            ])
            .unwrap();

        assert_eq!(heap.set_len(set), Some(2));
        assert_eq!(
            heap.set_snapshot(set),
            Some(vec![Value::Str("b".to_owned()), Value::Str("a".to_owned())])
        );
        assert_eq!(heap.stats().current_heap_units, 3);
        assert_eq!(
            heap.set_contains(set, &Value::Str("a".to_owned())),
            Some(true)
        );

        assert_eq!(heap.set_insert(set, Value::Str("c".to_owned())), Some(true));
        assert_eq!(heap.stats().current_heap_units, 4);
        assert_eq!(
            heap.set_insert(set, Value::Str("a".to_owned())),
            Some(false)
        );
        assert_eq!(heap.stats().current_heap_units, 4);
        assert_eq!(
            heap.set_remove(set, &Value::Str("b".to_owned())),
            Some(true)
        );
        assert_eq!(heap.stats().current_heap_units, 3);
        heap.set_clear(set).unwrap();
        assert_eq!(heap.set_snapshot(set), Some(vec![]));
        assert_eq!(heap.stats().current_heap_units, 1);
    }

    #[test]
    fn roots_are_explicit_storable_slots() {
        let heap = GcHeap::new(GcHeapConfig::default());
        let object = heap.alloc_array(vec![Value::I32(1)]).unwrap();
        let root = heap.root_value(Value::Array(object)).unwrap();

        assert_eq!(root.index(), 0);
        assert_eq!(heap.root_snapshot(root), Some(Value::Array(object)));
        assert_eq!(heap.active_roots(), 1);
        assert_eq!(heap.trace_roots(), vec![object]);

        assert!(heap.root_value(host_root_value(1)).is_none());
        assert!(heap.root_value(path_view_value(1)).is_none());
        assert!(
            heap.root_value(Value::Tuple(vec![shared_borrow_value(2)]))
                .is_none()
        );
        assert_eq!(heap.active_roots(), 1);
    }

    #[test]
    fn root_scanning_traces_only_gc_managed_boundaries() {
        let heap = GcHeap::new(GcHeapConfig::default());
        let leaf = heap.alloc_array(vec![Value::I32(1)]).unwrap();
        let map = heap
            .alloc_map(vec![(Value::Str("leaf".to_owned()), Value::Array(leaf))])
            .unwrap();
        let set = heap.alloc_set(vec![Value::Str("seen".to_owned())]).unwrap();
        let record = heap
            .alloc_struct(
                "Record".to_owned(),
                vec![StructValueField {
                    name: "map".to_owned(),
                    value: Value::Map(map),
                }],
            )
            .unwrap();

        let root = heap
            .root_value(Value::Tuple(vec![
                Value::Struct(record),
                Value::Set(set),
                Value::Unit,
            ]))
            .unwrap();

        assert_eq!(heap.trace_roots(), vec![record, map, leaf, set]);

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

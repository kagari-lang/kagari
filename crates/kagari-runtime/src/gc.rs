use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use indexmap::{IndexMap, IndexSet};

use crate::error::{RuntimeError, RuntimeErrorKind};
use crate::value::{EnumValueSnapshot, MapKey, StructValueField, Value};

#[derive(Debug, Clone, Copy)]
pub struct GcHeapConfig {
    /// Minimum live-unit threshold for collection at execution safepoints.
    pub collection_threshold: Option<usize>,
}

impl Default for GcHeapConfig {
    fn default() -> Self {
        Self {
            collection_threshold: Some(1024),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GcHeapStats {
    pub current_heap_units: usize,
    pub peak_heap_units: usize,
    pub allocation_units: usize,
    pub allocated_objects: usize,
    pub collections: u64,
    pub reclaimed_objects: usize,
    pub last_pause: Duration,
}

#[derive(Debug, Default)]
struct CollectorStats {
    allocated_objects: usize,
    collections: u64,
    reclaimed_objects: usize,
    last_pause: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeapObjectId {
    owner: u64,
    slot: usize,
    generation: u64,
}

impl HeapObjectId {
    pub fn index(self) -> usize {
        self.slot
    }
    pub fn generation(self) -> u64 {
        self.generation
    }
}

/// Owning root for host retention. Clones share one root; the last drop releases it.
#[must_use = "retain this handle for as long as the host needs the value"]
#[derive(Debug, Clone)]
pub struct RootedValue {
    roots: RootSet,
}
impl RootedValue {
    pub fn value(&self) -> Value {
        self.roots.get(0).expect("single root")
    }
    pub fn set(&self, heap: &GcHeap, value: Value) -> Option<()> {
        if !value.is_storable() {
            return None;
        }
        self.roots.set(heap, 0, value)
    }
}

/// A registered set of execution slots. Values in it remain live until last drop.
#[must_use = "retain the registered slots until execution resources are released"]
#[derive(Debug, Clone)]
pub struct RootSet {
    owner: u64,
    values: Rc<RefCell<Vec<Value>>>,
}
impl PartialEq for RootSet {
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner && Rc::ptr_eq(&self.values, &other.values)
    }
}
impl RootSet {
    pub fn get(&self, index: usize) -> Option<Value> {
        self.values.borrow().get(index).cloned()
    }
    pub fn set(&self, heap: &GcHeap, index: usize, value: Value) -> Option<()> {
        if self.owner != heap.owner || !heap.validate_value(&value) {
            return None;
        }
        *self.values.borrow_mut().get_mut(index)? = value;
        Some(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcCollection {
    pub reclaimed_objects: usize,
    pub reclaimed_units: usize,
    pub live_objects: usize,
    pub pause: Duration,
}

#[derive(Debug)]
struct ObjectSlot {
    generation: u64,
    initialization_owner: Option<crate::ModuleKey>,
    object: Option<HeapObject>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcObjectKind {
    Array,
    Map,
    Set,
    Enum,
    Struct,
}

#[derive(Debug)]
enum HeapObject {
    Array(Vec<Value>),
    Map(IndexMap<MapKey, Value>),
    Set(IndexSet<MapKey>),
    Enum(EnumValueSnapshot),
    Struct {
        layout: crate::module::StructLayoutRef,
        fields: Vec<Value>,
    },
}

impl HeapObject {
    fn units(&self) -> usize {
        1 + match self {
            Self::Array(values) => values.len(),
            Self::Map(values) => values.len(),
            Self::Set(values) => values.len(),
            Self::Enum(value) => value.fields.len(),
            Self::Struct { fields, .. } => fields.len(),
        }
    }
}

/// Keeps the iterated collection alive and blocks structural writes through aliases.
#[must_use = "retain the guard until iteration finishes"]
#[derive(Debug)]
pub struct CollectionIteration {
    active: Rc<RefCell<HashMap<HeapObjectId, usize>>>,
    id: HeapObjectId,
    _root: RootedValue,
}
impl Drop for CollectionIteration {
    fn drop(&mut self) {
        let mut active = self.active.borrow_mut();
        let count = active.get_mut(&self.id).expect("registered iteration");
        *count -= 1;
        if *count == 0 {
            active.remove(&self.id);
        }
    }
}

#[derive(Debug)]
pub struct GcHeap {
    owner: u64,
    config: GcHeapConfig,
    objects: RefCell<Vec<ObjectSlot>>,
    free: RefCell<Vec<usize>>,
    roots: RefCell<Vec<Weak<RefCell<Vec<Value>>>>>,
    stats: RefCell<CollectorStats>,
    resources: Rc<crate::resource::ResourceState>,
    next_collection: Cell<usize>,
    iterations: Rc<RefCell<HashMap<HeapObjectId, usize>>>,
}

impl GcHeap {
    pub(crate) fn resource_limit(&self, name: &'static str) -> RuntimeError {
        self.resources.limit(name)
    }
    pub(crate) fn commit_host_write(&self, commit: impl FnOnce()) -> Result<(), RuntimeError> {
        self.resources.commit_host_write(commit)
    }

    pub(crate) fn prepare_dirty_record(&self, current: usize) -> Result<(), RuntimeError> {
        self.resources.prepare_dirty_record(current)
    }

    pub(crate) fn ensure_execution_allowed(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()
    }
    pub fn new(config: GcHeapConfig, resources: Rc<crate::resource::ResourceState>) -> Self {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT_OWNER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .expect("heap identity exhausted");
        Self {
            owner,
            config,
            iterations: Default::default(),
            objects: RefCell::new(Vec::new()),
            free: RefCell::new(Vec::new()),
            roots: RefCell::new(Vec::new()),
            stats: RefCell::new(CollectorStats::default()),
            resources,
            next_collection: Cell::new(config.collection_threshold.unwrap_or(usize::MAX).max(1)),
        }
    }

    pub fn config(&self) -> GcHeapConfig {
        self.config
    }

    pub fn allocated_objects(&self) -> usize {
        self.stats.borrow().allocated_objects
    }

    pub fn stats(&self) -> GcHeapStats {
        let stats = self.stats.borrow();
        let counters = self.resources.counters();
        GcHeapStats {
            current_heap_units: counters.current_heap_units,
            peak_heap_units: counters.peak_heap_units,
            allocation_units: counters.allocation_units,
            allocated_objects: stats.allocated_objects,
            collections: stats.collections,
            reclaimed_objects: stats.reclaimed_objects,
            last_pause: stats.last_pause,
        }
    }

    pub fn active_roots(&self) -> usize {
        self.roots
            .borrow()
            .iter()
            .filter(|root| root.strong_count() > 0)
            .count()
    }

    pub fn alloc_array(&self, elements: Vec<Value>) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !elements.iter().all(|value| self.valid_payload(value)) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        self.alloc_object(HeapObject::Array(elements))
    }

    pub fn alloc_map(&self, entries: Vec<(Value, Value)>) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        let mut map = IndexMap::new();
        for (key, value) in entries {
            if !self.valid_payload(&value) {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "invalid heap target, index, or payload",
                ));
            }
            let key = MapKey::from_value(&key).ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key")
            })?;
            map.try_reserve(usize::from(!map.contains_key(&key)))
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            map.insert(key, value);
        }
        self.alloc_object(HeapObject::Map(map))
    }

    pub fn alloc_set(&self, values: Vec<Value>) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        let mut set = IndexSet::new();
        for value in values {
            let key = MapKey::from_value(&value).ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key")
            })?;
            set.try_reserve(usize::from(!set.contains(&key)))
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            set.insert(key);
        }
        self.alloc_object(HeapObject::Set(set))
    }

    pub(crate) fn alloc_struct(
        &self,
        layout: crate::module::StructLayoutRef,
        fields: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if fields.len() != layout.layout().fields.len()
            || !fields
                .iter()
                .zip(&layout.layout().fields)
                .all(|(value, field)| {
                    self.valid_payload(value) && value.has_representation(field.ty)
                })
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        self.alloc_object(HeapObject::Struct { layout, fields })
    }

    pub(crate) fn alloc_enum(
        &self,
        tag: crate::value::EnumTag,
        fields: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !tag.accepts_representations(&fields)
            || !fields.iter().all(|value| self.valid_payload(value))
            || matches!(&tag, crate::value::EnumTag::Declared(layout)
                if !fields.iter().zip(&layout.variant().payload).all(|(value, ty)| self.matches_abi(value, ty, layout.module())))
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        self.alloc_object(HeapObject::Enum(EnumValueSnapshot { tag, fields }))
    }

    fn matches_abi(
        &self,
        value: &Value,
        ty: &kagari_ir::module::abi::AbiType,
        owner: &crate::module::LoadedModule,
    ) -> bool {
        use crate::value::EnumTag;
        use kagari_ir::module::abi::AbiType;
        let mut pending = vec![(value.clone(), ty)];
        while let Some((value, ty)) = pending.pop() {
            match (value, ty) {
                (value, AbiType::Builtin(_)) if value.has_representation(ty.representation()) => {},
                (Value::Tuple(values), AbiType::Tuple(types)) if values.len() == types.len() => {
                    pending.extend(values.into_iter().zip(types));
                },
                (Value::Struct(id), AbiType::Struct(expected)) => {
                    if !self.struct_layout(id).is_some_and(|layout| owner.bytecode.structures.iter().any(|current| current.declaration == expected.declaration && current.arguments == expected.arguments && layout.layout() == current)) { return false; }
                },
                (Value::Enum(id), AbiType::Enum(expected)) => {
                    if !self.enum_snapshot(id).is_some_and(|value| matches!(value.tag, EnumTag::Declared(layout) if owner.bytecode.enumerations.iter().any(|current| current.declaration == expected.declaration && current.arguments == expected.arguments && layout.layout() == current))) { return false; }
                },
                (Value::Array(id), AbiType::Array(element)) => {
                    let Some(values) = self.array_snapshot(id) else { return false; };
                    pending.extend(values.into_iter().map(|value| (value, element.as_ref())));
                },
                (Value::Map(id), AbiType::Map { key, value }) => {
                    let Some(entries) = self.map_snapshot(id) else { return false; };
                    for (k, v) in entries { pending.push((k, key)); pending.push((v, value)); }
                },
                (Value::Set(id), AbiType::Set(element)) => {
                    let Some(values) = self.set_snapshot(id) else { return false; };
                    pending.extend(values.into_iter().map(|value| (value, element.as_ref())));
                },
                (Value::Enum(id), AbiType::StandardEnum { kind, args }) => {
                    let Some(snapshot) = self.enum_snapshot(id) else { return false; };
                    let index = match (kind, snapshot.tag) {
                        (kagari_ir::module::abi::StandardEnumKind::Option, EnumTag::OptionNone) => continue,
                        (kagari_ir::module::abi::StandardEnumKind::Option, EnumTag::OptionSome)
                        | (kagari_ir::module::abi::StandardEnumKind::Result, EnumTag::ResultOk) => 0,
                        (kagari_ir::module::abi::StandardEnumKind::Result, EnumTag::ResultErr) => 1,
                        _ => return false,
                    };
                    let Some(ty) = args.get(index) else { return false; };
                    pending.extend(snapshot.fields.into_iter().map(|value| (value, ty)));
                },
                _ => return false,
            }
        }
        true
    }

    pub fn begin_collection_iteration(
        &self,
        value: &Value,
    ) -> Result<CollectionIteration, RuntimeError> {
        self.ensure_execution_allowed()?;
        let id = match value {
            Value::Array(id) | Value::Map(id) | Value::Set(id) => *id,
            _ => {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "expected collection",
                ));
            }
        };
        let expected = match value {
            Value::Array(_) => GcObjectKind::Array,
            Value::Map(_) => GcObjectKind::Map,
            _ => GcObjectKind::Set,
        };
        if self.object_kind(id) != Some(expected) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid collection handle",
            ));
        }
        let root = self.root_value(value.clone()).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid collection handle")
        })?;
        let mut active = self.iterations.borrow_mut();
        let count = active
            .get(&id)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| self.resource_limit("iteration depth"))?;
        active
            .try_reserve(1)
            .map_err(|_| self.resource_limit("iteration registry"))?;
        active.insert(id, count);
        Ok(CollectionIteration {
            active: self.iterations.clone(),
            id,
            _root: root,
        })
    }

    pub(crate) fn ensure_structure_mutable(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        if self.iterations.borrow().contains_key(&id) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "structural modification during iteration",
            ));
        }
        Ok(())
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

    pub fn array_push(&self, id: HeapObjectId, value: Value) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        if !self.valid_payload(&value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        self.with_array_mut(id, |elements| {
            let growth = self.resources.prepare_heap_growth(1)?;
            elements
                .try_reserve(1)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            elements.push(value);
            growth.commit();
            Ok(())
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target"))?
    }

    pub fn array_pop(&self, id: HeapObjectId) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let value = self
            .with_array_mut(id, |elements| elements.pop())
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        if value.is_some() {
            self.release_heap_units(1);
        }
        Ok(value)
    }

    pub fn array_insert(
        &self,
        id: HeapObjectId,
        index: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        if !self.valid_payload(&value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        self.with_array_mut(id, |elements| {
            if index > elements.len() {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "invalid heap target, index, or payload",
                ));
            }
            let growth = self.resources.prepare_heap_growth(1)?;
            elements
                .try_reserve(1)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            elements.insert(index, value);
            growth.commit();
            Ok(())
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target"))?
    }

    pub fn array_remove(
        &self,
        id: HeapObjectId,
        index: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let value = self
            .with_array_mut(id, |elements| {
                (index < elements.len()).then(|| elements.remove(index))
            })
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        if value.is_some() {
            self.release_heap_units(1);
        }
        Ok(value)
    }

    pub fn array_clear(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let removed = self
            .with_array_mut(id, |elements| {
                let removed = elements.len();
                elements.clear();
                removed
            })
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        self.release_heap_units(removed);
        Ok(())
    }

    pub fn array_set(
        &self,
        id: HeapObjectId,
        index: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        if !self.valid_payload(&value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap payload",
            ));
        }
        self.with_array_mut(id, |elements| {
            let slot = elements.get_mut(index).ok_or_else(|| {
                RuntimeError::new(
                    RuntimeErrorKind::IndexOutOfBounds,
                    format!("invalid index `{index}`"),
                )
            })?;
            *slot = value;
            Ok(())
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target"))?
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

    pub fn map_insert(
        &self,
        id: HeapObjectId,
        key: Value,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        if !self.valid_payload(&value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        let key = MapKey::from_value(&key)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key"))?;
        self.with_map_mut(id, |entries| {
            let units = usize::from(!entries.contains_key(&key));
            if units != 0 {
                self.ensure_structure_mutable(id)?;
            }
            let growth = self.resources.prepare_heap_growth(units)?;
            entries
                .try_reserve(units)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            entries.insert(key, value);
            growth.commit();
            Ok(())
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target"))?
    }

    pub fn map_remove(&self, id: HeapObjectId, key: &Value) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let key = MapKey::from_value(key)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key"))?;
        let value = self
            .with_map_mut(id, |entries| entries.shift_remove(&key))
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        if value.is_some() {
            self.release_heap_units(1);
        }
        Ok(value)
    }

    pub fn map_clear(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let removed = self
            .with_map_mut(id, |entries| {
                let removed = entries.len();
                entries.clear();
                removed
            })
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        self.release_heap_units(removed);
        Ok(())
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

    pub fn set_insert(&self, id: HeapObjectId, value: Value) -> Result<bool, RuntimeError> {
        self.ensure_execution_allowed()?;
        let key = MapKey::from_value(&value)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key"))?;
        self.with_set_mut(id, |values| {
            let units = usize::from(!values.contains(&key));
            if units != 0 {
                self.ensure_structure_mutable(id)?;
            }
            let growth = self.resources.prepare_heap_growth(units)?;
            values
                .try_reserve(units)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            let inserted = values.insert(key);
            growth.commit();
            Ok(inserted)
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target"))?
    }

    pub fn set_remove(&self, id: HeapObjectId, value: &Value) -> Result<bool, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let key = MapKey::from_value(value)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key"))?;
        let removed = self
            .with_set_mut(id, |values| values.shift_remove(&key))
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        if removed {
            self.release_heap_units(1);
        }
        Ok(removed)
    }

    pub fn set_clear(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let removed = self
            .with_set_mut(id, |values| {
                let removed = values.len();
                values.clear();
                removed
            })
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        self.release_heap_units(removed);
        Ok(())
    }

    pub fn struct_layout(&self, id: HeapObjectId) -> Option<crate::module::StructLayoutRef> {
        self.with_struct(id, |layout, _| layout.clone())
    }
    pub fn struct_name(&self, id: HeapObjectId) -> Option<String> {
        self.with_struct(id, |layout, _| layout.layout().name().to_owned())
    }
    pub fn enum_snapshot(&self, id: HeapObjectId) -> Option<EnumValueSnapshot> {
        self.with_enum(id, Clone::clone)
    }
    pub fn struct_snapshot(&self, id: HeapObjectId) -> Option<(String, Vec<StructValueField>)> {
        self.with_struct(id, |layout, fields| {
            (
                layout.layout().name().to_owned(),
                fields
                    .iter()
                    .zip(&layout.layout().fields)
                    .map(|(value, field)| StructValueField {
                        name: field.name.clone(),
                        value: value.clone(),
                    })
                    .collect(),
            )
        })
    }
    pub fn struct_get_slot(
        &self,
        id: HeapObjectId,
        expected: &crate::module::StructLayoutRef,
        slot: usize,
    ) -> Option<Value> {
        self.with_struct(id, |layout, fields| {
            if !layout.matches(expected) {
                return None;
            }
            fields.get(slot).cloned()
        })
        .flatten()
    }
    pub fn struct_set_slot(
        &self,
        id: HeapObjectId,
        expected: &crate::module::StructLayoutRef,
        slot: usize,
        next_value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        if !self.valid_payload(&next_value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap payload",
            ));
        }
        self.with_struct_mut(id, |layout, fields| {
            if !layout.matches(expected) {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "struct layout mismatch",
                ));
            }
            let field = layout.layout().fields.get(slot).ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid field slot")
            })?;
            if !field.mutable || !next_value.has_representation(field.ty) {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "field is read-only or value has the wrong representation",
                ));
            }
            let target = fields.get_mut(slot).ok_or_else(|| {
                self.resources
                    .quarantine("struct storage does not match its layout")
            })?;
            *target = next_value;
            Ok(())
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target"))?
    }

    pub fn object_kind(&self, id: HeapObjectId) -> Option<GcObjectKind> {
        let objects = self.objects.borrow();
        match self.object_ref(&objects, id)? {
            HeapObject::Array(_) => Some(GcObjectKind::Array),
            HeapObject::Map(_) => Some(GcObjectKind::Map),
            HeapObject::Set(_) => Some(GcObjectKind::Set),
            HeapObject::Enum(_) => Some(GcObjectKind::Enum),
            HeapObject::Struct { .. } => Some(GcObjectKind::Struct),
        }
    }

    pub fn root_value(&self, value: Value) -> Option<RootedValue> {
        if !value.is_storable() {
            return None;
        }
        Some(RootedValue {
            roots: self.root_execution_values(vec![value])?,
        })
    }

    pub fn root_execution_values(&self, values: Vec<Value>) -> Option<RootSet> {
        self.ensure_execution_allowed().ok()?;
        if !values.iter().all(|value| self.validate_value(value)) {
            return None;
        }
        let values = Rc::new(RefCell::new(values));
        let mut roots = self.roots.borrow_mut();
        roots.retain(|root| root.strong_count() > 0);
        roots.push(Rc::downgrade(&values));
        Some(RootSet {
            owner: self.owner,
            values,
        })
    }

    pub fn trace_roots(&self) -> Option<Vec<HeapObjectId>> {
        self.trace_values(&self.root_snapshots())
    }

    pub fn trace_value(&self, value: &Value) -> Option<Vec<HeapObjectId>> {
        self.trace_values(std::slice::from_ref(value))
    }

    fn root_snapshots(&self) -> Vec<Value> {
        self.roots
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .flat_map(|root| root.borrow().clone())
            .collect()
    }

    pub fn collection_due(&self) -> bool {
        self.config.collection_threshold.is_some()
            && self.resources.counters().current_heap_units >= self.next_collection.get()
    }

    /// Stop-the-world, nonmoving collection. Additional roots belong to runtime
    /// module state; registered host/frame roots are always included.
    pub(crate) fn collect(&self, additional_roots: &[Value]) -> Option<GcCollection> {
        let started = Instant::now();
        let mut values = self.root_snapshots();
        values.extend_from_slice(additional_roots);
        let live = self
            .trace_values(&values)?
            .into_iter()
            .collect::<HashSet<_>>();
        let mut reclaimed_objects = 0;
        let mut reclaimed_units = 0;
        let mut objects = self.objects.borrow_mut();
        let mut free = self.free.borrow_mut();
        for (index, slot) in objects.iter_mut().enumerate() {
            let id = HeapObjectId {
                owner: self.owner,
                slot: index,
                generation: slot.generation,
            };
            if !live.contains(&id)
                && let Some(object) = slot.object.take()
            {
                reclaimed_objects += 1;
                reclaimed_units += object.units();
                if let Some(generation) = slot.generation.checked_add(1) {
                    slot.generation = generation;
                    free.push(index);
                }
            }
        }
        drop(objects);
        self.release_heap_units(reclaimed_units);
        self.roots
            .borrow_mut()
            .retain(|root| root.strong_count() > 0);
        let pause = started.elapsed();
        let mut stats = self.stats.borrow_mut();
        stats.collections += 1;
        stats.reclaimed_objects += reclaimed_objects;
        stats.allocated_objects -= reclaimed_objects;
        stats.last_pause = pause;
        self.next_collection.set(
            self.resources
                .counters()
                .current_heap_units
                .saturating_mul(2)
                .max(self.config.collection_threshold.unwrap_or(usize::MAX))
                .max(1),
        );
        Some(GcCollection {
            reclaimed_objects,
            reclaimed_units,
            live_objects: live.len(),
            pause,
        })
    }

    pub fn validate_value(&self, value: &Value) -> bool {
        if matches!(
            value,
            Value::Unit
                | Value::Bool(_)
                | Value::I32(_)
                | Value::I64(_)
                | Value::F32(_)
                | Value::F64(_)
                | Value::Str(_)
                | Value::Interface(_)
                | Value::HostRoot(_)
                | Value::Ephemeral(_)
        ) {
            return true;
        }
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            let expected = match value {
                Value::HostPathView(view) => {
                    pending.extend(view.dynamic_args().as_slice().iter().map(|arg| &arg.value));
                    continue;
                }
                Value::Tuple(elements) => {
                    pending.extend(elements);
                    continue;
                }
                Value::Array(id) => (*id, Some(GcObjectKind::Array)),
                Value::Map(id) => (*id, Some(GcObjectKind::Map)),
                Value::Set(id) => (*id, Some(GcObjectKind::Set)),
                Value::Enum(id) => (*id, Some(GcObjectKind::Enum)),
                Value::Struct(id) => (*id, Some(GcObjectKind::Struct)),
                Value::GcHandle(id) => (*id, None),
                _ => continue,
            };
            let Some(actual) = self.object_kind(expected.0) else {
                return false;
            };
            if expected.1.is_some_and(|kind| actual != kind) {
                return false;
            }
        }
        true
    }

    pub(crate) fn validate_candidate_value(&self, value: &Value) -> bool {
        let Some(session) = self.resources.active_session().filter(|session| {
            session.options.phase == crate::ExecutionPhase::CandidateInitialization
        }) else {
            return true;
        };
        self.validate_candidate_value_for(session.root.program_root().key(), value)
    }

    pub(crate) fn validate_candidate_value_for(
        &self,
        owner: crate::ModuleKey,
        value: &Value,
    ) -> bool {
        if !value.is_default_heap_payload() {
            return false;
        }
        let Some(references) = self.trace_value(value) else {
            return false;
        };
        let objects = self.objects.borrow();
        references.into_iter().all(|id| {
            let Some(object) = self.object_ref(&objects, id) else {
                return false;
            };
            matches!(object, HeapObject::Enum(_))
                || objects[id.slot].initialization_owner == Some(owner)
        })
    }

    fn valid_payload(&self, value: &Value) -> bool {
        value.is_default_heap_payload() && self.validate_value(value)
    }

    fn alloc_object(&self, object: HeapObject) -> Result<HeapObjectId, RuntimeError> {
        let growth = self.resources.prepare_heap_growth(object.units())?;
        let initialization_owner = self
            .resources
            .active_session()
            .filter(|session| {
                session.options.phase == crate::ExecutionPhase::CandidateInitialization
            })
            .map(|session| session.root.program_root().key());
        let mut objects = self.objects.borrow_mut();
        let slot = if let Some(index) = self.free.borrow_mut().pop() {
            objects[index].object = Some(object);
            objects[index].initialization_owner = initialization_owner;
            index
        } else {
            objects
                .try_reserve(1)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            let index = objects.len();
            objects.push(ObjectSlot {
                generation: 0,
                initialization_owner,
                object: Some(object),
            });
            index
        };
        self.stats.borrow_mut().allocated_objects += 1;
        growth.commit();
        Ok(HeapObjectId {
            owner: self.owner,
            slot,
            generation: objects[slot].generation,
        })
    }

    fn object_ref<'a>(
        &self,
        objects: &'a [ObjectSlot],
        id: HeapObjectId,
    ) -> Option<&'a HeapObject> {
        if id.owner != self.owner {
            return None;
        }
        let slot = objects.get(id.slot)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.object.as_ref()
    }
    // Script-visible reads are isolated; collector traversal uses object_ref directly.
    fn readable_object<'a>(
        &self,
        objects: &'a [ObjectSlot],
        id: HeapObjectId,
    ) -> Option<&'a HeapObject> {
        let object = self.object_ref(objects, id)?;
        if let Some(session) = self.resources.active_session().filter(|session| {
            session.options.phase == crate::ExecutionPhase::CandidateInitialization
        }) && objects[id.slot].initialization_owner != Some(session.root.program_root().key())
        {
            return None;
        }
        Some(object)
    }

    fn object_mut<'a>(
        &self,
        objects: &'a mut [ObjectSlot],
        id: HeapObjectId,
    ) -> Option<&'a mut HeapObject> {
        if id.owner != self.owner {
            return None;
        }
        let slot = objects.get_mut(id.slot)?;
        if slot.generation != id.generation {
            return None;
        }
        if let Some(session) = self.resources.active_session().filter(|session| {
            session.options.phase == crate::ExecutionPhase::CandidateInitialization
        }) && slot.initialization_owner != Some(session.root.program_root().key())
        {
            return None;
        }
        slot.object.as_mut()
    }

    fn release_heap_units(&self, units: usize) {
        self.resources.release_heap_units(units);
    }

    fn trace_values(&self, values: &[Value]) -> Option<Vec<HeapObjectId>> {
        if !values.iter().all(|value| self.validate_value(value)) {
            return None;
        }
        let objects = self.objects.borrow();
        let mut seen = HashSet::new();
        let mut traced = Vec::new();
        let mut pending = values.iter().rev().collect::<Vec<_>>();
        while let Some(value) = pending.pop() {
            let id = match value {
                Value::HostPathView(view) => {
                    pending.extend(
                        view.dynamic_args()
                            .as_slice()
                            .iter()
                            .rev()
                            .map(|arg| &arg.value),
                    );
                    continue;
                }
                Value::Tuple(elements) => {
                    pending.extend(elements.iter().rev());
                    continue;
                }
                Value::Array(id)
                | Value::Map(id)
                | Value::Set(id)
                | Value::Enum(id)
                | Value::Struct(id)
                | Value::GcHandle(id) => *id,
                _ => continue,
            };
            let object = self.object_ref(&objects, id)?;
            if !seen.insert(id) {
                continue;
            }
            traced.push(id);
            match object {
                HeapObject::Array(elements) => pending.extend(elements.iter().rev()),
                HeapObject::Map(entries) => pending.extend(entries.values().rev()),
                HeapObject::Enum(snapshot) => pending.extend(snapshot.fields.iter().rev()),
                HeapObject::Struct { fields, .. } => pending.extend(fields.iter().rev()),
                HeapObject::Set(_) => {}
            }
        }
        Some(traced)
    }

    fn with_array<R>(&self, id: HeapObjectId, f: impl FnOnce(&Vec<Value>) -> R) -> Option<R> {
        let objects = self.objects.borrow();
        match self.readable_object(&objects, id)? {
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
        match self.object_mut(&mut objects, id)? {
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
        match self.readable_object(&objects, id)? {
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
        match self.object_mut(&mut objects, id)? {
            HeapObject::Map(entries) => Some(f(entries)),
            HeapObject::Array(_)
            | HeapObject::Set(_)
            | HeapObject::Enum(_)
            | HeapObject::Struct { .. } => None,
        }
    }

    fn with_set<R>(&self, id: HeapObjectId, f: impl FnOnce(&IndexSet<MapKey>) -> R) -> Option<R> {
        let objects = self.objects.borrow();
        match self.readable_object(&objects, id)? {
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
        match self.object_mut(&mut objects, id)? {
            HeapObject::Set(values) => Some(f(values)),
            HeapObject::Array(_)
            | HeapObject::Map(_)
            | HeapObject::Enum(_)
            | HeapObject::Struct { .. } => None,
        }
    }

    fn with_enum<R>(&self, id: HeapObjectId, f: impl FnOnce(&EnumValueSnapshot) -> R) -> Option<R> {
        let objects = self.objects.borrow();
        match self.object_ref(&objects, id)? {
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
        f: impl FnOnce(&crate::module::StructLayoutRef, &Vec<Value>) -> R,
    ) -> Option<R> {
        let objects = self.objects.borrow();
        match self.readable_object(&objects, id)? {
            HeapObject::Struct { layout, fields } => Some(f(layout, fields)),
            HeapObject::Array(_)
            | HeapObject::Map(_)
            | HeapObject::Set(_)
            | HeapObject::Enum(_) => None,
        }
    }

    fn with_struct_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&crate::module::StructLayoutRef, &mut Vec<Value>) -> R,
    ) -> Option<R> {
        let mut objects = self.objects.borrow_mut();
        match self.object_mut(&mut objects, id)? {
            HeapObject::Struct { layout, fields } => Some(f(layout, fields)),
            HeapObject::Array(_)
            | HeapObject::Map(_)
            | HeapObject::Set(_)
            | HeapObject::Enum(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use kagari_ir::module::ValueType;
    fn layout(name: &str, field: &str, ty: ValueType) -> crate::module::StructLayoutRef {
        crate::layout_fixtures::layout(&mut crate::Runtime::default(), name, &[(field, ty, true)])
    }
    use super::*;
    use crate::{
        host::{
            DynamicPathArguments, HostBorrowTable, HostObjectId, HostPathDescriptorRegistration,
            HostPathSegmentRegistration, HostRootHandle, HostSchemaEpoch, HostTypeOwnership,
        },
        metadata::{AbiFingerprint, PathAccess, TypeId},
    };

    fn host_root_value(object_id: u64) -> Value {
        Value::HostRoot(HostRootHandle::new(
            Default::default(),
            HostObjectId(object_id),
            TypeId::new(0),
            HostSchemaEpoch::new(0),
            AbiFingerprint(1),
        ))
    }

    fn path_view_value(object_id: u64) -> Value {
        let result_type = TypeId::new(1);
        let mut runtime = crate::Runtime::default();
        let mut declaration = kagari_common::host_interface::HostTypeDeclaration::new("Player");
        declaration.ownership = HostTypeOwnership::HostRoot;
        declaration.path_access = PathAccess::ReadWrite;
        let mut hp = kagari_common::host_interface::HostFieldDeclaration::new(
            &declaration.id,
            "hp",
            kagari_common::host_interface::HostValueType::I32,
        );
        hp.writable = true;
        hp.path_access = PathAccess::ReadWrite;
        declaration.fields.push(hp);
        let root_type = runtime
            .register_host_type(crate::HostTypeRegistration::new(declaration, "Player"))
            .unwrap();
        let root = runtime
            .register_host_root(HostObjectId(object_id), root_type, HostSchemaEpoch::new(0))
            .unwrap();
        let descriptor = runtime
            .register_host_path_descriptor(HostPathDescriptorRegistration {
                root_type,
                result_type,
                segments: vec![HostPathSegmentRegistration::Field {
                    declaration: runtime
                        .host()
                        .host_type(root_type)
                        .unwrap()
                        .declaration
                        .fields
                        .iter()
                        .find(|field| field.name == "hp")
                        .unwrap()
                        .id
                        .clone(),
                }],
                access: PathAccess::ReadWrite,
                schema_epoch: HostSchemaEpoch::new(0),
                capability_requirements: crate::security::CapabilitySet::default(),
            })
            .unwrap();
        Value::HostPathView(
            runtime
                .host()
                .make_path_view(root, descriptor, DynamicPathArguments::empty())
                .unwrap(),
        )
    }

    fn shared_borrow_value(object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame().unwrap();
        Value::host_ref(
            guard
                .borrow_shared(HostObjectId(object_id), TypeId::new(0))
                .unwrap(),
        )
    }

    fn unique_borrow_value(object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame().unwrap();
        Value::host_mut(
            guard
                .borrow_unique(HostObjectId(object_id), TypeId::new(0))
                .unwrap(),
        )
    }

    #[test]
    fn rejects_ephemeral_values_as_heap_payloads() {
        let heap = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );

        assert!(heap.alloc_array(vec![shared_borrow_value(1)]).is_err());
        assert!(heap.alloc_array(vec![unique_borrow_value(2)]).is_err());
        assert_eq!(heap.allocated_objects(), 0);
    }

    #[test]
    fn rejects_host_handles_and_path_views_as_default_heap_payloads() {
        let heap = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );

        assert!(heap.alloc_array(vec![host_root_value(1)]).is_err());
        assert!(
            heap.alloc_map(vec![(Value::Str("host".to_owned()), host_root_value(2))])
                .is_err()
        );
        assert!(
            heap.alloc_map(vec![(path_view_value(3), Value::I32(1))])
                .is_err()
        );
        assert!(heap.alloc_set(vec![host_root_value(4)]).is_err());
        assert!(
            heap.alloc_struct(
                layout("HostBacked", "path", ValueType::HeapObject),
                vec![path_view_value(3)],
            )
            .is_err()
        );
        assert_eq!(heap.allocated_objects(), 0);
    }

    #[test]
    fn rejects_non_storable_heap_mutations() {
        let heap = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
        let array = heap.alloc_array(vec![Value::I32(1)]).unwrap();
        let record = heap
            .alloc_struct(
                layout("Record", "value", ValueType::I32),
                vec![Value::I32(1)],
            )
            .unwrap();

        assert!(heap.array_push(array, shared_borrow_value(1)).is_err());
        assert!(heap.array_set(array, 0, path_view_value(4)).is_err());
        assert!(
            heap.struct_set_slot(
                record,
                &heap.struct_layout(record).unwrap(),
                0,
                host_root_value(5)
            )
            .is_err()
        );

        assert_eq!(heap.array_snapshot(array), Some(vec![Value::I32(1)]));
        assert_eq!(
            heap.struct_get_slot(record, &heap.struct_layout(record).unwrap(), 0),
            Some(Value::I32(1))
        );
    }

    #[test]
    fn assigns_stable_object_identity_and_kind() {
        let heap = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
        let first = heap.alloc_array(vec![]).unwrap();
        let second = heap.alloc_map(vec![]).unwrap();
        let third = heap.alloc_set(vec![]).unwrap();
        let fourth = heap
            .alloc_struct(layout("Empty", "value", ValueType::Unit), vec![Value::Unit])
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
        let heap = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
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
            heap.map_remove(map, &Value::Str("b".to_owned())).unwrap(),
            Some(Value::I32(3))
        );
        assert_eq!(heap.stats().current_heap_units, 3);
        heap.map_clear(map).unwrap();
        assert_eq!(heap.map_snapshot(map), Some(vec![]));
        assert_eq!(heap.stats().current_heap_units, 1);
    }

    #[test]
    fn builtin_ordered_sets_preserve_insertion_order_and_account_units() {
        let heap = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
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

        assert_eq!(heap.set_insert(set, Value::Str("c".to_owned())), Ok(true));
        assert_eq!(heap.stats().current_heap_units, 4);
        assert_eq!(heap.set_insert(set, Value::Str("a".to_owned())), Ok(false));
        assert_eq!(heap.stats().current_heap_units, 4);
        assert_eq!(heap.set_remove(set, &Value::Str("b".to_owned())), Ok(true));
        assert_eq!(heap.stats().current_heap_units, 3);
        heap.set_clear(set).unwrap();
        assert_eq!(heap.set_snapshot(set), Some(vec![]));
        assert_eq!(heap.stats().current_heap_units, 1);
    }

    #[test]
    fn roots_are_explicit_storable_slots() {
        let heap = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
        let object = heap.alloc_array(vec![Value::I32(1)]).unwrap();
        let root = heap.root_value(Value::Array(object)).unwrap();

        assert_eq!(root.value(), Value::Array(object));
        assert_eq!(heap.active_roots(), 1);
        assert_eq!(heap.trace_roots().unwrap(), vec![object]);

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
        let heap = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
        let leaf = heap.alloc_array(vec![Value::I32(1)]).unwrap();
        let map = heap
            .alloc_map(vec![(Value::Str("leaf".to_owned()), Value::Array(leaf))])
            .unwrap();
        let set = heap.alloc_set(vec![Value::Str("seen".to_owned())]).unwrap();
        let record = heap
            .alloc_struct(
                layout("Record", "map", ValueType::HeapObject),
                vec![Value::Map(map)],
            )
            .unwrap();

        let root = heap
            .root_value(Value::Tuple(vec![
                Value::Struct(record),
                Value::Set(set),
                Value::Unit,
            ]))
            .unwrap();

        assert_eq!(heap.trace_roots().unwrap(), vec![record, map, leaf, set]);

        root.set(&heap, Value::GcHandle(leaf)).unwrap();
        assert_eq!(heap.trace_roots().unwrap(), vec![leaf]);

        assert_eq!(root.value(), Value::GcHandle(leaf));
        drop(root);
        assert_eq!(heap.trace_roots().unwrap(), Vec::<HeapObjectId>::new());
    }

    #[test]
    fn root_scanning_handles_cycles_without_duplicate_identity() {
        let heap = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
        let array = heap.alloc_array(vec![]).unwrap();
        let record = heap
            .alloc_struct(
                layout("Cycle", "array", ValueType::HeapObject),
                vec![Value::Array(array)],
            )
            .unwrap();
        heap.array_push(array, Value::Struct(record)).unwrap();
        let _root = heap.root_value(Value::Array(array)).unwrap();

        assert_eq!(heap.trace_roots().unwrap(), vec![array, record]);
    }
    #[test]
    fn removal_results_distinguish_absence_from_iteration_and_stale_handle_errors() {
        let heap = GcHeap::new(Default::default(), Default::default());
        let array = heap.alloc_array(vec![]).unwrap();
        let map = heap.alloc_map(vec![]).unwrap();
        let set = heap.alloc_set(vec![]).unwrap();
        assert_eq!(heap.array_pop(array).unwrap(), None);
        assert_eq!(heap.array_remove(array, 0).unwrap(), None);
        assert_eq!(heap.map_remove(map, &Value::I32(1)).unwrap(), None);
        assert!(!heap.set_remove(set, &Value::I32(1)).unwrap());
        assert!(heap.map_remove(map, &Value::Tuple(vec![])).is_err());
        assert!(heap.set_remove(set, &Value::Tuple(vec![])).is_err());
        let guards = [Value::Array(array), Value::Map(map), Value::Set(set)]
            .map(|value| heap.begin_collection_iteration(&value).unwrap());
        let before = heap.stats().current_heap_units;
        for result in [
            heap.array_pop(array).map(|_| ()),
            heap.array_remove(array, 0).map(|_| ()),
            heap.array_clear(array),
            heap.map_remove(map, &Value::I32(1)).map(|_| ()),
            heap.map_clear(map),
            heap.set_remove(set, &Value::I32(1)).map(|_| ()),
            heap.set_clear(set),
        ] {
            let error = result.unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
            assert_eq!(error.message(), "structural modification during iteration");
        }
        assert_eq!(heap.stats().current_heap_units, before);
        drop(guards);
        heap.array_clear(array).unwrap();
        heap.map_clear(map).unwrap();
        heap.set_clear(set).unwrap();
        heap.collect(&[]).unwrap();
        assert!(heap.array_pop(array).is_err());
        assert!(heap.array_remove(array, 0).is_err());
        assert!(heap.array_clear(array).is_err());
        assert!(heap.map_remove(map, &Value::I32(1)).is_err());
        assert!(heap.map_clear(map).is_err());
        assert!(heap.set_remove(set, &Value::I32(1)).is_err());
        assert!(heap.set_clear(set).is_err());
    }
    #[test]
    fn replacement_errors_preserve_targets_and_internal_fault_categories() {
        let resources = Rc::new(crate::resource::ResourceState::default());
        let heap = GcHeap::new(Default::default(), resources.clone());
        let array = heap.alloc_array(vec![Value::I32(1)]).unwrap();
        let before = heap.stats().current_heap_units;
        let foreign_heap = GcHeap::new(Default::default(), Default::default());
        let foreign = foreign_heap.alloc_array(vec![]).unwrap();
        // Payload and receiver rejection must not be relabeled from the index.
        assert_eq!(
            heap.array_set(array, usize::MAX, Value::Array(foreign))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ScriptTrap
        );
        assert_eq!(
            heap.array_set(foreign, usize::MAX, Value::I32(9))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ScriptTrap
        );

        assert_eq!(
            heap.array_set(array, 2, Value::I32(9)).unwrap_err().kind(),
            RuntimeErrorKind::IndexOutOfBounds
        );
        assert_eq!(heap.array_get(array, 0), Some(Value::I32(1)));
        assert_eq!(heap.stats().current_heap_units, before);
        let schema = layout("Record", "value", ValueType::I32);
        let object = heap
            .alloc_struct(schema.clone(), vec![Value::I32(7)])
            .unwrap();
        assert_eq!(
            heap.struct_set_slot(object, &schema, 0, Value::Bool(false))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ScriptTrap
        );
        assert_eq!(
            heap.struct_get_slot(object, &schema, 0),
            Some(Value::I32(7))
        );
        // Simulate a broken engine invariant rather than a script type error.
        heap.with_struct_mut(object, |_, fields| fields.clear())
            .unwrap();
        let error =
            crate::reflection::set_field(&heap, &Value::Struct(object), "value", Value::I32(9))
                .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
        assert_eq!(
            error.into_write_error().kind(),
            RuntimeErrorKind::EngineFault
        );
        assert!(resources.is_quarantined());
        assert_eq!(
            heap.array_set(array, 0, Value::I32(9)).unwrap_err().kind(),
            RuntimeErrorKind::EngineFault
        );
        assert_eq!(heap.array_get(array, 0), Some(Value::I32(1)));
        let error = crate::reflection::set_index(
            &heap,
            &Value::Array(array),
            &Value::I32(0),
            Value::I32(9),
        )
        .unwrap_err();
        assert_eq!(
            error.into_write_error().kind(),
            RuntimeErrorKind::EngineFault
        );
    }
}

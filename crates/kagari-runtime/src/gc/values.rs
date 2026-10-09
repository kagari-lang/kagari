//! Immutable value payloads and scoped host descriptors share checked heap lifetime.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId, storage::HeapObject},
    host::{FrameHostBorrowToken, HostPathViewHandle, HostRootHandle},
    range::RangeValue,
    value::{EphemeralValue, Value},
};
use std::{cell::Ref, sync::Arc};

#[derive(Debug, Clone, Copy)]
pub(crate) struct TupleProperties {
    pub(crate) storable: bool,
    pub(crate) ephemeral: bool,
    pub(crate) host_borrow: bool,
}

#[derive(Debug)]
pub(super) struct TupleData {
    pub(super) members: Vec<Value>,
    pub(super) properties: TupleProperties,
}

impl GcHeap {
    pub fn alloc_string(&self, text: String) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.alloc_object(HeapObject::String(text)).map(Value::Str)
    }

    /// The storage borrow prevents collection or mutation until the view is dropped.
    /// Release it before calling script/native code or creating another heap value.
    pub fn string(&self, id: HeapObjectId) -> Option<Ref<'_, str>> {
        Ref::filter_map(self.objects.try_borrow().ok()?, |objects| {
            match self.object_ref(objects, id)? {
                HeapObject::String(text) => Some(text.as_str()),
                _ => None,
            }
        })
        .ok()
    }

    /// Construct immutable membership; updates publish another tuple sharing its members.
    pub fn alloc_tuple(&self, members: Vec<Value>) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !members.iter().all(|value| self.validate_value(value)) {
            return Err(RuntimeError::module_validation("invalid tuple member"));
        }
        // Immutable membership makes these construction-time facts stable. They
        // avoid revisiting a shared aggregate graph on every publication check.
        let properties = TupleProperties {
            storable: members.iter().all(|value| value.is_storable(self)),
            ephemeral: members.iter().any(|value| value.contains_ephemeral(self)),
            host_borrow: members.iter().any(|value| value.contains_host_borrow(self)),
        };
        self.alloc_object(HeapObject::Tuple(TupleData {
            members,
            properties,
        }))
        .map(Value::Tuple)
    }

    pub fn tuple(&self, id: HeapObjectId) -> Option<Ref<'_, [Value]>> {
        Ref::filter_map(self.objects.try_borrow().ok()?, |objects| {
            match self.object_ref(objects, id)? {
                HeapObject::Tuple(tuple) => Some(tuple.members.as_slice()),
                _ => None,
            }
        })
        .ok()
    }

    pub(crate) fn tuple_properties(&self, id: HeapObjectId) -> Option<TupleProperties> {
        let objects = self.objects.try_borrow().ok()?;
        match self.object_ref(&objects, id)? {
            HeapObject::Tuple(tuple) => Some(tuple.properties),
            _ => None,
        }
    }

    pub fn alloc_range(&self, range: RangeValue) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.alloc_object(HeapObject::Range(range))
            .map(Value::Range)
    }

    pub fn range(&self, id: HeapObjectId) -> Option<RangeValue> {
        let objects = self.objects.try_borrow().ok()?;
        match self.object_ref(&objects, id)? {
            HeapObject::Range(range) => Some(*range),
            _ => None,
        }
    }

    pub fn alloc_host_root(&self, root: HostRootHandle) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.alloc_object(HeapObject::HostRoot(root))
            .map(Value::HostRoot)
    }

    pub fn host_root(&self, id: HeapObjectId) -> Option<HostRootHandle> {
        let objects = self.objects.try_borrow().ok()?;
        match self.object_ref(&objects, id)? {
            HeapObject::HostRoot(root) => Some(*root),
            _ => None,
        }
    }

    pub fn alloc_host_path(&self, path: HostPathViewHandle) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !path
            .retained_values()
            .all(|value| value.is_storable(self) && self.validate_value(value))
        {
            return Err(RuntimeError::module_validation(
                "invalid host path argument",
            ));
        }
        self.alloc_object(HeapObject::HostPath(Arc::new(path)))
            .map(Value::HostPathView)
    }

    /// The returned descriptor does not root its dynamic argument values.
    /// Keep the containing Value in a live execution or host root while using it.
    pub fn host_path(&self, id: HeapObjectId) -> Option<Arc<HostPathViewHandle>> {
        let objects = self.objects.try_borrow().ok()?;
        match self.object_ref(&objects, id)? {
            HeapObject::HostPath(path) => Some(path.clone()),
            _ => None,
        }
    }

    pub fn alloc_ephemeral(&self, value: EphemeralValue) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        if let EphemeralValue::Runtime(id) = value {
            return Ok(Value::RuntimeEphemeral(id));
        }
        self.alloc_object(HeapObject::Ephemeral(value))
            .map(Value::Ephemeral)
    }

    pub fn ephemeral(&self, id: HeapObjectId) -> Option<EphemeralValue> {
        let objects = self.objects.try_borrow().ok()?;
        match self.object_ref(&objects, id)? {
            HeapObject::Ephemeral(value) => Some(*value),
            _ => None,
        }
    }

    pub fn alloc_host_ref(&self, token: FrameHostBorrowToken) -> Result<Value, RuntimeError> {
        self.alloc_ephemeral(EphemeralValue::HostRef(token))
    }

    pub fn alloc_host_mut(&self, token: FrameHostBorrowToken) -> Result<Value, RuntimeError> {
        self.alloc_ephemeral(EphemeralValue::HostMut(token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{ObjectSlot, roots::RootedValue};
    use std::mem::size_of;

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn scalar_slot_layout_does_not_inline_host_descriptors() {
        // An interpreter storage budget, not a serialized or external ABI.
        fn assert_copy<T: Copy>() {}

        assert_copy::<Value>();
        #[cfg(not(debug_assertions))]
        assert!(size_of::<Value>() <= 16);
        assert_eq!(size_of::<HeapObjectId>(), 12);
        eprintln!(
            "Value={} HeapObjectId={} HeapObject={} ObjectSlot={} TupleData={} String={} Range={} Ephemeral={} RootedValue={} HostRoot={} HostPath={} HostBorrow={}",
            size_of::<Value>(),
            size_of::<HeapObjectId>(),
            size_of::<HeapObject>(),
            size_of::<ObjectSlot>(),
            size_of::<TupleData>(),
            size_of::<String>(),
            size_of::<RangeValue>(),
            size_of::<EphemeralValue>(),
            size_of::<RootedValue>(),
            size_of::<HostRootHandle>(),
            size_of::<HostPathViewHandle>(),
            size_of::<FrameHostBorrowToken>(),
        );
    }
}

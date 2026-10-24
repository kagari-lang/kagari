//! Common retained key protocol and checked access for maps and sets.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::{HeapObjectId, custom_keys::KeyLookupRoots, roots::RootedValue},
    native::{
        binding::NativeResult, conversion::context::ConversionContext,
        function_handle::PreparedFunction, stored_selection::StoredSelection, typed::NativeContext,
    },
    value::Value,
};
use kagari_types::collection::CollectionAccess;
use std::{slice, sync::Arc};

#[derive(Debug, Clone)]
pub(super) struct HashHandle {
    pub(super) root: RootedValue,
    pub(super) key: TypeArgument,
    pub(super) access: CollectionAccess,
    protocol: Option<Arc<KeyProtocol>>,
}

#[derive(Debug)]
struct KeyProtocol {
    hash: StoredSelection,
    equal: StoredSelection,
}

impl HashHandle {
    pub(super) fn new(
        cx: &ConversionContext<'_>,
        value: &Value,
        key: TypeArgument,
        access: CollectionAccess,
    ) -> NativeResult<Self> {
        let id = match value {
            Value::Map(id) | Value::Set(id) => *id,
            _ => return Err(RuntimeError::module_validation("hash collection value")),
        };
        let root = cx
            .runtime()
            .root_value(*value)
            .ok_or_else(|| RuntimeError::module_validation("hash collection retention"))?;
        let selected = cx.runtime().gc().native_selections(id)?;
        let protocol = match selected.as_ref() {
            [] => {
                cx.runtime().gc().ensure_key_mode(value, false)?;
                None
            }
            [hash, equal] => {
                cx.runtime().gc().ensure_key_mode(value, true)?;
                Some(Arc::new(KeyProtocol {
                    hash: hash.clone(),
                    equal: equal.clone(),
                }))
            }
            _ => {
                return Err(RuntimeError::module_validation(
                    "hash collection selected protocol",
                ));
            }
        };
        Ok(Self {
            root,
            key,
            access,
            protocol,
        })
    }

    pub(super) fn value(
        &self,
        cx: &NativeContext<'_>,
        write: bool,
    ) -> NativeResult<(Value, HeapObjectId)> {
        cx.runtime().gc().ensure_no_native_borrow()?;
        cx.runtime().resources().ensure_execution_allowed()?;
        cx.poll()?;
        if write && self.access != CollectionAccess::Mutable {
            return Err(RuntimeError::module_validation(
                "collection view is read-only",
            ));
        }
        let value = self.root.value(cx.runtime().gc()).ok_or_else(|| {
            RuntimeError::module_validation("foreign or expired collection handle")
        })?;
        match value {
            Value::Map(id) | Value::Set(id) => Ok((value, id)),
            _ => Err(RuntimeError::module_validation("hash collection handle")),
        }
    }

    pub(super) fn into_value(
        self,
        cx: &ConversionContext<'_>,
        expected: &TypeArgument,
        access: CollectionAccess,
    ) -> NativeResult<Value> {
        if access == CollectionAccess::Mutable && self.access != CollectionAccess::Mutable {
            return Err(RuntimeError::module_validation(
                "read-only collection cannot become mutable",
            ));
        }
        let value = self
            .root
            .value(cx.runtime().gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired collection"))?;
        cx.check_value(expected, &value)?;
        Ok(value)
    }

    /// The collection/key stay rooted and aliases cannot mutate the table while
    /// user hashing/equality runs. No heap borrow crosses a script call.
    pub(super) fn lookup(
        &self,
        cx: &NativeContext<'_>,
        collection: &Value,
        key: &Value,
    ) -> NativeResult<Option<(i64, i64)>> {
        let Some(protocol) = &self.protocol else {
            return Ok(None);
        };
        let heap = cx.runtime().gc();
        let roots = heap
            .root_execution_values(vec![*collection, *key])
            .ok_or_else(|| RuntimeError::module_validation("key lookup retention"))?;
        let _lookup = heap.begin_key_lookup(collection, KeyLookupRoots::Host(&roots))?;
        // The rooted collection owns the stored evidence. Select current bodies
        // only for this synchronous lookup, never for the retained handle's lifetime.
        let hash = PreparedFunction::selected(
            cx.runtime(),
            cx.conversion.owner(),
            protocol.hash.retain(cx.runtime())?,
        )?;
        let equal = PreparedFunction::selected(
            cx.runtime(),
            cx.conversion.owner(),
            protocol.equal.retain(cx.runtime())?,
        )?;
        let hash = hash.call_values(cx, slice::from_ref(key))?;
        let Some(Value::I64(hash)) = hash.value(heap) else {
            return Err(RuntimeError::module_validation("Hash result must be i64"));
        };
        let mut index = 0;
        while let Some((token, stored)) = heap.custom_candidate(collection, hash, index)? {
            cx.poll()?;
            let equal = equal.call_values(cx, &[stored, *key])?;
            match equal.value(heap) {
                Some(Value::Bool(true)) => return Ok(Some((hash, token))),
                Some(Value::Bool(false)) => {}
                _ => return Err(RuntimeError::module_validation("Eq result must be bool")),
            }
            index += 1;
        }
        Ok(Some((hash, -1)))
    }
}

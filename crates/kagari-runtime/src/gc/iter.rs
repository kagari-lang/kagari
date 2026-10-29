use super::string_iter::StringTraversal;

use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    gc::{GcHeap, GcObjectKind, reference::GcRef, storage::HeapObject},
    module::ExecutableModule,
    native::{
        storage::{
            NativePayload, NativeStorage,
            scan::{NativeScanKind, ScanBudget, ScanStatus},
            value::StoredValue,
        },
        storage_type::StorageType,
    },
    resource::HeapCharge,
    value::Value,
    value_check::matches_type,
};

use kagari_contract::operations::StringIterKind;
use kagari_types::{declaration::native::NativeStorageLayout, scalar::BuiltinType, ty::Ty};
use std::slice;

struct IterTypeScope<'a> {
    source: &'a TypeArgument,
    item: StorageType,
}

/// Owned preview data, materialized only after releasing iterator storage views.
pub(super) enum PendingItem {
    Value(Value),
    Text(String),
    Pair(Value, Value),
}

impl PendingItem {
    fn materialize(self, heap: &GcHeap) -> Result<Value, RuntimeError> {
        match self {
            Self::Value(value) => Ok(value),
            Self::Text(text) => heap.alloc_string(text),
            Self::Pair(a, b) => heap.alloc_tuple(vec![a, b]),
        }
    }
}

#[derive(Debug)]
pub(super) struct NativeIter {
    pub(super) source: StoredValue,
    pub(super) item_type: Ty,
    pub(super) item_contract: StorageType,
    pub(super) position: u128,
    pub(super) string: Option<StringTraversal>,
    pub(super) revision: u64,
}

unsafe impl NativePayload for NativeIter {
    type ScanState = (usize, usize);
    const SCAN_KIND: NativeScanKind = NativeScanKind::Resumable;

    fn begin_scan(&self) -> Self::ScanState {
        (0, 2)
    }

    fn scan<'payload>(
        &'payload self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'payload StoredValue),
    ) -> ScanStatus {
        while state.0 < state.1 {
            if !budget.take(1) {
                return ScanStatus::Pending;
            }
            match state.0 {
                0 => visit(&self.source),
                1 => {
                    if let Some(string) = &self.string {
                        string.trace(visit);
                    }
                }
                _ => unreachable!("bounded builtin cursor scan"),
            }
            state.0 += 1;
        }
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(1, 0)
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "invalid or structurally modified iterator",
    )
}

impl GcHeap {
    /// Structural revision for the builtin Array, String, and Range cursor.
    /// Element replacement does not change this revision. Invalid handles and
    /// objects under exclusive native operations have no readable revision.
    fn source_revision(&self, source: &Value) -> Option<u64> {
        self.ensure_execution_allowed().ok()?;
        match source {
            Value::Str(_) | Value::Range(_) => Some(0),

            Value::Array(id) => {
                self.check_native_operation(*id, None, false).ok()?;
                let objects = self.objects.borrow();
                let HeapObject::Native(object) = self.readable_identity(&objects, *id)? else {
                    return None;
                };

                Some(object.revision)
            }
            _ => None,
        }
    }

    pub(crate) fn matches_iter_type(&self, id: GcRef, element: TypeView<'_>) -> bool {
        let objects = self.objects.borrow();
        matches!(self.readable_identity(&objects, id), Some(HeapObject::Native(object)) if matches!(object.ty(), Ty::Iter(_)) && object.payload::<NativeIter>().is_ok_and(|iter| iter.item_contract.matches_view(element)))
    }

    pub(crate) fn new_iter(
        &self,
        source: &Value,
        ty: &TypeArgument,
        item: TypeArgument,
        owner: &ExecutableModule,
    ) -> Result<Value, RuntimeError> {
        let scope = IterTypeScope {
            source: ty,
            item: StorageType::prepare_scoped(item, owner)?,
        };
        self.new_iter_with(source, ty.ty(), None, owner, Some(scope))
    }

    fn new_iter_with(
        &self,
        source: &Value,
        ty: &Ty,
        traversal: Option<(Ty, StringTraversal)>,
        owner: &ExecutableModule,
        scope: Option<IterTypeScope<'_>>,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        let valid = match (source, ty) {
            (Value::Array(id), Ty::Array(_)) => {
                self.identity_kind(*id) == Some(GcObjectKind::Array)
            }

            (Value::Range(range), Ty::Range(_, kind)) => {
                kind.has_start() && self.range(*range).is_some_and(|range| range.matches(ty))
            }
            (Value::Str(_), Ty::Builtin(BuiltinType::String)) => true,
            _ => false,
        };
        if !valid
            || !match &scope {
                Some(scope) => scope.source.matches_heap(self, source, owner),
                None => matches_type(self, source, ty, owner),
            }
        {
            return Err(invalid());
        }
        let item_type = match ty {
            Ty::Range(item, _) | Ty::Array(item) => (**item).clone(),
            Ty::Builtin(BuiltinType::String) => ty.clone(),
            _ => return Err(invalid()),
        };
        let (item_type, string) = match traversal {
            Some((item, traversal)) => (item, Some(traversal)),
            None => (item_type, None),
        };
        let item_contract = match scope {
            Some(scope) if *scope.item.ty() == item_type => scope.item,
            Some(_) => return Err(invalid()),
            None => StorageType::prepare(item_type.clone(), owner)?,
        };
        let revision = self.source_revision(source).ok_or_else(invalid)?;
        let cursor_type = Ty::Iter(Box::new(item_type.clone()));
        let payload = NativeIter {
            source: StoredValue::new(*source),
            item_type,
            item_contract,
            position: 0,
            string,
            revision,
        };
        let object = self
            .cursor_storage
            .get_or_init(|| {
                NativeStorage::provided_with_layout::<NativeIter>(NativeStorageLayout::Iterator {
                    item: 0,
                })
            })
            .prepare_payload(self, &cursor_type, payload, owner)?;
        let id = self.alloc_native(object)?;
        Ok(Value::GcHandle(id))
    }

    pub(crate) fn new_string_iter(
        &self,
        source: &Value,
        ty: &Ty,
        kind: StringIterKind,
        owner: &ExecutableModule,
    ) -> Result<Value, RuntimeError> {
        if !kind.valid_source(ty) {
            return Err(invalid());
        }
        let (source, traversal) = match (ty, source) {
            (Ty::Builtin(BuiltinType::String), Value::Str(_)) => (
                *source,
                StringTraversal::new(kind, slice::from_ref(source))?,
            ),
            (Ty::Tuple(_), Value::Tuple(fields)) => {
                let fields = self.tuple(*fields).ok_or_else(invalid)?;
                let traversal = StringTraversal::new(kind, &fields)?;
                (fields[0], traversal)
            }
            _ => return Err(invalid()),
        };
        self.new_iter_with(
            &source,
            &Ty::Builtin(BuiltinType::String),
            Some((kind.item_type(), traversal)),
            owner,
            None,
        )
    }

    pub(crate) fn advance_iter_with<R>(
        &self,
        value: &Value,
        ty: &Ty,
        finish: impl FnOnce(Option<Value>) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        self.ensure_execution_allowed()?;
        let (Value::GcHandle(id), Ty::Iter(item)) = (value, ty) else {
            return Err(invalid());
        };
        self.check_native_operation(*id, None, true)?;
        let (payload, next_position, string_cursor) = {
            let objects = self.objects.borrow();
            let Some(object) = self.native_ref(&objects, *id) else {
                return Err(invalid());
            };
            let iter = object.payload::<NativeIter>()?;
            if iter.item_type != **item
                || self.source_revision(iter.source.value_ref()) != Some(iter.revision)
            {
                return Err(invalid());
            }
            let mut string_cursor = None;
            let (payload, advance) = if let Some(traversal) = &iter.string {
                let Value::Str(text) = iter.source.value_ref() else {
                    return Err(invalid());
                };
                let (value, cursor) =
                    traversal.preview(self, &self.string(*text).ok_or_else(invalid)?)?;
                string_cursor = Some(cursor);
                (value, 0)
            } else {
                match iter.source.value_ref() {
                    Value::Range(range) => (
                        self.range(*range)
                            .ok_or_else(invalid)?
                            .at(iter.position)?
                            .map(PendingItem::Value),
                        1,
                    ),
                    Value::Array(id) => (
                        self.buffer_get(*id, iter.position).map(PendingItem::Value),
                        1,
                    ),

                    Value::Str(text) => {
                        let text = self.string(*text).ok_or_else(invalid)?;
                        let character = usize::try_from(iter.position)
                            .ok()
                            .and_then(|position| text.get(position..))
                            .and_then(|tail| tail.chars().next());
                        match character {
                            Some(character) => (
                                Some(PendingItem::Text(character.to_string())),
                                character.len_utf8() as u128,
                            ),
                            None => (None, 0),
                        }
                    }
                    _ => return Err(invalid()),
                }
            };
            (
                payload,
                iter.position.checked_add(advance).ok_or_else(invalid)?,
                string_cursor,
            )
        };
        let payload = payload.map(|item| item.materialize(self)).transpose()?;
        let _payload_root = payload
            .as_ref()
            .filter(|value| value.is_managed())
            .map(|value| self.root_value(*value).ok_or_else(invalid))
            .transpose()?;
        let has_item = payload.is_some();
        let result = finish(payload)?;
        self.check_native_operation(*id, None, true)?;
        let mut objects = self.objects_mut()?;
        let Some(object) = self.native_mut(&mut objects, *id) else {
            return Err(invalid());
        };
        let iter = object.payload_mut::<NativeIter>()?;
        if let (Some(traversal), Some(cursor)) = (&mut iter.string, string_cursor) {
            traversal.cursor = cursor;
        }
        if has_item {
            iter.position = next_position;
        }
        Ok(result)
    }
}

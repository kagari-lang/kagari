use crate::{error::VmError, executor::Executor};
use kagari_bytecode::instruction::Slot;
use kagari_runtime::value::{EnumTag, Value};
use kagari_types::ty::Ty;

impl Executor<'_> {
    pub(crate) fn test_enum_variant(&self, value: Slot) -> Result<Value, VmError> {
        let Value::Enum(handle) = unsafe {
            self.current_frame()?
                .read_slot_transport(self.runtime, value)
        }?
        else {
            return Err(VmError::TypeMismatch("enum pattern expects enum value"));
        };
        // SAFETY: The current frame retains the source; this short view ends before
        // result publication or any callback.
        let view = unsafe { self.runtime.gc().enum_view_transport(handle) }
            .ok_or(VmError::TypeMismatch("invalid enum handle"))?;
        let expected = self.current_frame()?.enum_variant(self.runtime)?;
        Ok(Value::Bool(matches!(
            &view.tag,
            EnumTag::Declared(actual) if actual.matches_layout(&expected)
        )))
    }

    pub(crate) fn read_enum_payload(&self, value: Slot, index: u32) -> Result<Value, VmError> {
        let Value::Enum(handle) = unsafe {
            self.current_frame()?
                .read_slot_transport(self.runtime, value)
        }?
        else {
            return Err(VmError::TypeMismatch("enum pattern expects enum value"));
        };
        // SAFETY: The current frame retains the source; this short view ends before
        // result publication or any callback.
        let view = unsafe { self.runtime.gc().enum_view_transport(handle) }
            .ok_or(VmError::TypeMismatch("invalid enum handle"))?;
        let expected = self.current_frame()?.enum_variant(self.runtime)?;
        if !matches!(&view.tag, EnumTag::Declared(actual) if actual.matches_layout(&expected)) {
            return Err(VmError::TypeMismatch("enum pattern variant mismatch"));
        }
        view.fields
            .get(index as usize)
            .copied()
            .ok_or(VmError::TypeMismatch("invalid enum payload index"))
    }

    pub(crate) fn make_enum(&self, fields: &[Slot]) -> Result<Value, VmError> {
        let fields = fields
            .iter()
            .map(|register| {
                Ok::<_, VmError>(unsafe {
                    self.current_frame()?
                        .read_slot_transport(self.runtime, *register)
                }?)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let layout = self.current_frame()?.enum_variant(self.runtime)?;
        unsafe {
            self.runtime
                .alloc_enum_transport(EnumTag::Declared(layout), fields)
        }
        .map(Value::Enum)
        .map_err(VmError::RuntimeError)
    }

    pub(crate) fn make_tuple(&self, elements: &[Slot]) -> Result<Value, VmError> {
        elements
            .iter()
            .map(|element| {
                Ok::<_, VmError>(unsafe {
                    self.current_frame()?
                        .read_slot_transport(self.runtime, *element)
                }?)
            })
            .collect::<Result<Vec<_>, _>>()
            .and_then(|values| {
                unsafe { self.runtime.gc().alloc_tuple_transport(values) }.map_err(VmError::from)
            })
    }

    pub(crate) fn make_array(&self, element: &Ty, elements: &[Slot]) -> Result<Value, VmError> {
        let elements = elements
            .iter()
            .map(|element| {
                Ok::<_, VmError>(unsafe {
                    self.current_frame()?
                        .read_slot_transport(self.runtime, *element)
                }?)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !elements.iter().all(|value| {
            // SAFETY: Source slots in the current frame retain every element.
            unsafe { value.is_default_heap_payload_transport(self.runtime.gc()) }
        }) {
            return Err(VmError::TypeMismatch(
                "make_array expects default-storable elements",
            ));
        }
        let handle = unsafe {
            self.current_frame()?
                .alloc_array_transport(self.runtime, element, elements)
        }
        .map_err(VmError::RuntimeError)?;
        Ok(Value::Array(handle))
    }

    pub(crate) fn make_struct(&self, fields: &[Slot]) -> Result<Value, VmError> {
        let fields = fields
            .iter()
            .map(|field| {
                Ok::<_, VmError>(unsafe {
                    self.current_frame()?
                        .read_slot_transport(self.runtime, *field)
                }?)
            })
            .collect::<Result<Vec<_>, VmError>>()?;
        let layout = self.current_frame()?.struct_layout(self.runtime)?;
        let handle = unsafe { self.runtime.alloc_struct_transport(layout, fields) }
            .map_err(VmError::RuntimeError)?;
        Ok(Value::Struct(handle))
    }
}

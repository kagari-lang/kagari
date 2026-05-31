use kagari_ir::bytecode::{Register, StructFieldInit};
use kagari_runtime::{
    RuntimeError,
    value::{StructValueField, Value},
};

use crate::error::VmError;
use crate::executor::Executor;

impl Executor<'_> {
    pub(crate) fn make_tuple(&self, elements: &[Register]) -> Result<Value, VmError> {
        elements
            .iter()
            .map(|element| self.current_frame()?.read_register(*element))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Tuple)
    }

    pub(crate) fn make_array(&self, elements: &[Register]) -> Result<Value, VmError> {
        let elements = elements
            .iter()
            .map(|element| self.current_frame()?.read_register(*element))
            .collect::<Result<Vec<_>, _>>()?;
        let elements_are_payload = elements.iter().all(Value::is_default_heap_payload);
        let handle = self.runtime.gc().alloc_array(elements).ok_or_else(|| {
            if elements_are_payload {
                VmError::RuntimeError(RuntimeError::resource_limit("heap units"))
            } else {
                VmError::TypeMismatch("make_array expects default-storable elements")
            }
        })?;
        self.runtime
            .sync_heap_accounting()
            .map_err(VmError::RuntimeError)?;
        Ok(Value::Array(handle))
    }

    pub(crate) fn make_struct(
        &self,
        name: String,
        fields: &[StructFieldInit],
    ) -> Result<Value, VmError> {
        let fields = fields
            .iter()
            .map(|field| {
                Ok(StructValueField {
                    name: field.name.clone(),
                    value: self.current_frame()?.read_register(field.value)?,
                })
            })
            .collect::<Result<Vec<_>, VmError>>()?;
        let fields_are_payload = fields
            .iter()
            .all(|field| field.value.is_default_heap_payload());

        let handle = self
            .runtime
            .gc()
            .alloc_struct(name, fields)
            .ok_or_else(|| {
                if fields_are_payload {
                    VmError::RuntimeError(RuntimeError::resource_limit("heap units"))
                } else {
                    VmError::TypeMismatch("make_struct expects default-storable fields")
                }
            })?;
        self.runtime
            .sync_heap_accounting()
            .map_err(VmError::RuntimeError)?;
        Ok(Value::Struct(handle))
    }

    pub(crate) fn read_field(&self, base: Register, name: &str) -> Result<Value, VmError> {
        match self.current_frame()?.read_register(base)? {
            Value::Struct(handle) => self
                .runtime
                .gc()
                .struct_get_field(handle, name)
                .ok_or_else(|| VmError::MissingField(name.to_owned())),
            _ => Err(VmError::TypeMismatch("read_field expects struct value")),
        }
    }

    pub(crate) fn read_index(&self, base: Register, index: Register) -> Result<Value, VmError> {
        let base = self.current_frame()?.read_register(base)?;
        let index = self.current_frame()?.read_register(index)?;
        let index = match index {
            Value::I32(index) if index >= 0 => index as usize,
            Value::I64(index) if index >= 0 => index as usize,
            _ => {
                return Err(VmError::TypeMismatch(
                    "read_index expects non-negative integer index",
                ));
            }
        };

        match base {
            Value::Array(handle) => self
                .runtime
                .gc()
                .array_get(handle, index)
                .ok_or(VmError::InvalidIndex(index)),
            Value::Tuple(elements) => elements
                .get(index)
                .cloned()
                .ok_or(VmError::InvalidIndex(index)),
            _ => Err(VmError::TypeMismatch(
                "read_index expects array or tuple value",
            )),
        }
    }

    pub(crate) fn write_field(
        &self,
        base: Register,
        name: &str,
        value: Register,
    ) -> Result<(), VmError> {
        let value = self.current_frame()?.read_register(value)?;
        if !value.is_default_heap_payload() {
            return Err(VmError::TypeMismatch(
                "write_field expects default-storable value",
            ));
        }
        match self.current_frame()?.read_register(base)? {
            Value::Struct(handle) => self
                .runtime
                .gc()
                .struct_set_field(handle, name, value)
                .ok_or_else(|| VmError::MissingField(name.to_owned())),
            _ => Err(VmError::TypeMismatch("write_field expects struct value")),
        }
    }

    pub(crate) fn write_index(
        &mut self,
        base: Register,
        index: Register,
        value: Register,
    ) -> Result<(), VmError> {
        let base_value = self.current_frame()?.read_register(base)?;
        let index_value = self.current_frame()?.read_register(index)?;
        let value = self.current_frame()?.read_register(value)?;
        let index = match index_value {
            Value::I32(index) if index >= 0 => index as usize,
            Value::I64(index) if index >= 0 => index as usize,
            _ => {
                return Err(VmError::TypeMismatch(
                    "write_index expects non-negative integer index",
                ));
            }
        };

        match base_value {
            Value::Array(handle) => {
                if !value.is_default_heap_payload() {
                    return Err(VmError::TypeMismatch(
                        "write_index expects default-storable value",
                    ));
                }
                self.runtime
                    .gc()
                    .array_set(handle, index, value)
                    .ok_or(VmError::InvalidIndex(index))
            }
            Value::Tuple(mut elements) => {
                let Some(slot) = elements.get_mut(index) else {
                    return Err(VmError::InvalidIndex(index));
                };
                *slot = value;
                self.current_frame_mut()?
                    .write_register(base, Value::Tuple(elements))
            }
            _ => Err(VmError::TypeMismatch(
                "write_index expects array or tuple value",
            )),
        }
    }
}

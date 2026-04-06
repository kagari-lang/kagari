use kagari_ir::bytecode::{Register, StructFieldInit};
use kagari_runtime::value::{StructValueField, Value};

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
        elements
            .iter()
            .map(|element| self.current_frame()?.read_register(*element))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array)
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

        Ok(Value::Struct { name, fields })
    }

    pub(crate) fn read_field(&self, base: Register, name: &str) -> Result<Value, VmError> {
        match self.current_frame()?.read_register(base)? {
            Value::Struct { fields, .. } => fields
                .into_iter()
                .find(|field| field.name == name)
                .map(|field| field.value)
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
            Value::Array(elements) | Value::Tuple(elements) => elements
                .get(index)
                .cloned()
                .ok_or(VmError::InvalidIndex(index)),
            _ => Err(VmError::TypeMismatch(
                "read_index expects array or tuple value",
            )),
        }
    }
}

use crate::{error::VmError, executor::Executor};
use kagari_bytecode::instruction::Register;
use kagari_common::identity::table::DefinitionId;
use kagari_runtime::value::{EnumTag, Value};
use kagari_types::ty::Ty;

impl Executor<'_> {
    pub(crate) fn test_enum_variant(&self, value: Register) -> Result<Value, VmError> {
        let Value::Enum(handle) = self.current_frame()?.read_register(self.runtime, value)? else {
            return Err(VmError::TypeMismatch("enum pattern expects enum value"));
        };
        let view = self
            .runtime
            .gc()
            .enum_view(handle)
            .ok_or(VmError::TypeMismatch("invalid enum handle"))?;
        let expected = self.current_frame()?.enum_variant(self.runtime)?;
        Ok(Value::Bool(matches!(
            &view.tag,
            EnumTag::Declared(actual) if actual.matches_layout(&expected)
        )))
    }

    pub(crate) fn read_enum_payload(&self, value: Register, index: u32) -> Result<Value, VmError> {
        let Value::Enum(handle) = self.current_frame()?.read_register(self.runtime, value)? else {
            return Err(VmError::TypeMismatch("enum pattern expects enum value"));
        };
        let view = self
            .runtime
            .gc()
            .enum_view(handle)
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

    pub(crate) fn make_enum(&self, fields: &[Register]) -> Result<Value, VmError> {
        let fields = fields
            .iter()
            .map(|register| {
                Ok::<_, VmError>(
                    self.current_frame()?
                        .read_register(self.runtime, *register)?,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let layout = self.current_frame()?.enum_variant(self.runtime)?;
        self.runtime
            .alloc_enum(EnumTag::Declared(layout), fields)
            .map(Value::Enum)
            .map_err(VmError::RuntimeError)
    }

    pub(crate) fn make_tuple(&self, elements: &[Register]) -> Result<Value, VmError> {
        elements
            .iter()
            .map(|element| {
                Ok::<_, VmError>(
                    self.current_frame()?
                        .read_register(self.runtime, *element)?,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .and_then(|values| self.runtime.gc().alloc_tuple(values).map_err(VmError::from))
    }

    pub(crate) fn make_array(
        &self,
        element: &Ty<DefinitionId>,
        elements: &[Register],
    ) -> Result<Value, VmError> {
        let elements = elements
            .iter()
            .map(|element| {
                Ok::<_, VmError>(
                    self.current_frame()?
                        .read_register(self.runtime, *element)?,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !elements
            .iter()
            .all(|value| value.is_default_heap_payload(self.runtime.gc()))
        {
            return Err(VmError::TypeMismatch(
                "make_array expects default-storable elements",
            ));
        }
        let handle = self
            .current_frame()?
            .alloc_array(self.runtime, element, elements)
            .map_err(VmError::RuntimeError)?;
        Ok(Value::Array(handle))
    }

    pub(crate) fn make_struct(&self, fields: &[Register]) -> Result<Value, VmError> {
        let fields = fields
            .iter()
            .map(|field| {
                Ok::<_, VmError>(self.current_frame()?.read_register(self.runtime, *field)?)
            })
            .collect::<Result<Vec<_>, VmError>>()?;
        let layout = self.current_frame()?.struct_layout(self.runtime)?;
        let handle = self
            .runtime
            .alloc_struct(layout, fields)
            .map_err(VmError::RuntimeError)?;
        Ok(Value::Struct(handle))
    }
}

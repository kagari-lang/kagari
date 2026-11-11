use crate::{
    error::{RuntimeError, trap::TrapReason},
    executor::Interpreter,
    value::{EnumTag, Value},
};

use kagari_bytecode::instruction::Slot;
use kagari_types::ty::Ty;

impl Interpreter<'_> {
    pub(crate) fn test_enum_variant(&self, value: Slot) -> Result<Value, RuntimeError> {
        let Value::Enum(handle) = self.current_frame()?.read_slot(self.runtime, value)? else {
            return Err(RuntimeError::from(TrapReason::Provider(
                ("enum pattern expects enum value").into(),
            )));
        };
        let view = self
            .runtime
            .gc()
            .enum_view(handle)
            .ok_or(RuntimeError::from(TrapReason::Provider(
                ("invalid enum handle").into(),
            )))?;
        let expected = self.current_frame()?.enum_variant(self.runtime)?;
        Ok(Value::Bool(matches!(
            &view.tag,
            EnumTag::Declared(actual) if actual.matches_layout(&expected)
        )))
    }

    pub(crate) fn read_enum_payload(&self, value: Slot, index: u32) -> Result<Value, RuntimeError> {
        let Value::Enum(handle) = self.current_frame()?.read_slot(self.runtime, value)? else {
            return Err(RuntimeError::from(TrapReason::Provider(
                ("enum pattern expects enum value").into(),
            )));
        };
        let view = self
            .runtime
            .gc()
            .enum_view(handle)
            .ok_or(RuntimeError::from(TrapReason::Provider(
                ("invalid enum handle").into(),
            )))?;
        let expected = self.current_frame()?.enum_variant(self.runtime)?;
        if !matches!(&view.tag, EnumTag::Declared(actual) if actual.matches_layout(&expected)) {
            return Err(RuntimeError::from(TrapReason::Provider(
                ("enum pattern variant mismatch").into(),
            )));
        }
        view.fields
            .get(index as usize)
            .copied()
            .ok_or(RuntimeError::from(TrapReason::Provider(
                ("invalid enum payload index").into(),
            )))
    }

    pub(crate) fn make_enum(&self, fields: &[Slot]) -> Result<Value, RuntimeError> {
        let fields = fields
            .iter()
            .map(|register| self.current_frame()?.read_slot(self.runtime, *register))
            .collect::<Result<Vec<_>, _>>()?;
        let layout = self.current_frame()?.enum_variant(self.runtime)?;
        self.runtime
            .alloc_enum(EnumTag::Declared(layout), fields)
            .map(Value::Enum)
    }

    pub(crate) fn make_tuple(&self, elements: &[Slot]) -> Result<Value, RuntimeError> {
        elements
            .iter()
            .map(|element| self.current_frame()?.read_slot(self.runtime, *element))
            .collect::<Result<Vec<_>, _>>()
            .and_then(|values| self.runtime.gc().alloc_tuple(values))
    }

    pub(crate) fn make_array(
        &self,
        element: &Ty,
        elements: &[Slot],
    ) -> Result<Value, RuntimeError> {
        let elements = elements
            .iter()
            .map(|element| self.current_frame()?.read_slot(self.runtime, *element))
            .collect::<Result<Vec<_>, _>>()?;
        if !elements
            .iter()
            .all(|value| value.is_default_heap_payload(self.runtime.gc()))
        {
            return Err(RuntimeError::from(TrapReason::Provider(
                ("make_array expects default-storable elements").into(),
            )));
        }
        let handle = self
            .current_frame()?
            .alloc_array(self.runtime, element, elements)?;
        Ok(Value::Array(handle))
    }

    pub(crate) fn make_struct(&self, fields: &[Slot]) -> Result<Value, RuntimeError> {
        let fields = fields
            .iter()
            .map(|field| self.current_frame()?.read_slot(self.runtime, *field))
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        let layout = self.current_frame()?.struct_layout(self.runtime)?;
        let handle = self.runtime.alloc_struct(layout, fields)?;
        Ok(Value::Struct(handle))
    }
}

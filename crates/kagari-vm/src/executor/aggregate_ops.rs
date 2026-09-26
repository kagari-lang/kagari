use kagari_ir::bytecode::{FieldRef, Register, StructId};
use kagari_runtime::value::Value;

use crate::error::VmError;
use crate::executor::Executor;

impl Executor<'_> {
    pub(crate) fn test_enum_variant(
        &self,
        value: Register,
        enumeration: kagari_ir::bytecode::EnumId,
        variant: u32,
    ) -> Result<Value, VmError> {
        let Value::Enum(handle) = self.current_frame()?.read_register(value)? else {
            return Err(VmError::TypeMismatch("enum pattern expects enum value"));
        };
        let snapshot = self
            .runtime
            .gc()
            .enum_snapshot(handle)
            .ok_or(VmError::TypeMismatch("invalid enum handle"))?;
        let expected = self
            .current_loaded()?
            .enum_variant(enumeration, variant)
            .ok_or(VmError::TypeMismatch("invalid enum pattern layout"))?;
        Ok(Value::Bool(matches!(
            snapshot.tag,
            kagari_runtime::value::EnumTag::Declared(actual) if actual == expected
        )))
    }

    pub(crate) fn read_enum_payload(
        &self,
        value: Register,
        enumeration: kagari_ir::bytecode::EnumId,
        variant: u32,
        index: u32,
    ) -> Result<Value, VmError> {
        let Value::Enum(handle) = self.current_frame()?.read_register(value)? else {
            return Err(VmError::TypeMismatch("enum pattern expects enum value"));
        };
        let snapshot = self
            .runtime
            .gc()
            .enum_snapshot(handle)
            .ok_or(VmError::TypeMismatch("invalid enum handle"))?;
        let expected = self
            .current_loaded()?
            .enum_variant(enumeration, variant)
            .ok_or(VmError::TypeMismatch("invalid enum pattern layout"))?;
        if !matches!(snapshot.tag, kagari_runtime::value::EnumTag::Declared(actual) if actual == expected)
        {
            return Err(VmError::TypeMismatch("enum pattern variant mismatch"));
        }
        snapshot
            .fields
            .get(index as usize)
            .cloned()
            .ok_or(VmError::TypeMismatch("invalid enum payload index"))
    }

    pub(crate) fn make_enum(
        &self,
        enumeration: kagari_ir::bytecode::EnumId,
        variant: u32,
        fields: &[Register],
    ) -> Result<Value, VmError> {
        let fields = fields
            .iter()
            .map(|register| Ok::<_, VmError>(self.current_frame()?.read_register(*register)?))
            .collect::<Result<Vec<_>, _>>()?;
        let layout = self
            .current_loaded()?
            .enum_variant(enumeration, variant)
            .ok_or(VmError::TypeMismatch("invalid enum variant layout"))?;
        self.runtime
            .alloc_enum(kagari_runtime::value::EnumTag::Declared(layout), fields)
            .map(Value::Enum)
            .map_err(VmError::RuntimeError)
    }

    pub(crate) fn make_tuple(&self, elements: &[Register]) -> Result<Value, VmError> {
        elements
            .iter()
            .map(|element| Ok::<_, VmError>(self.current_frame()?.read_register(*element)?))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Tuple)
    }

    pub(crate) fn make_array(&self, elements: &[Register]) -> Result<Value, VmError> {
        let elements = elements
            .iter()
            .map(|element| Ok::<_, VmError>(self.current_frame()?.read_register(*element)?))
            .collect::<Result<Vec<_>, _>>()?;
        if !elements.iter().all(Value::is_default_heap_payload) {
            return Err(VmError::TypeMismatch(
                "make_array expects default-storable elements",
            ));
        }
        let handle = self
            .runtime
            .alloc_array(elements)
            .map_err(VmError::RuntimeError)?;
        Ok(Value::Array(handle))
    }

    pub(crate) fn make_struct(
        &self,
        structure: StructId,
        fields: &[Register],
    ) -> Result<Value, VmError> {
        let fields = fields
            .iter()
            .map(|field| Ok::<_, VmError>(self.current_frame()?.read_register(*field)?))
            .collect::<Result<Vec<_>, VmError>>()?;
        let layout = self
            .current_loaded()?
            .struct_layout(structure)
            .ok_or(VmError::TypeMismatch("invalid struct layout"))?;
        let handle = self
            .runtime
            .alloc_struct(layout, fields)
            .map_err(VmError::RuntimeError)?;
        Ok(Value::Struct(handle))
    }

    pub(crate) fn read_field(&self, base: Register, field: FieldRef) -> Result<Value, VmError> {
        let layout = self
            .current_loaded()?
            .struct_layout(field.structure)
            .ok_or(VmError::TypeMismatch("invalid struct layout"))?;
        match self.current_frame()?.read_register(base)? {
            Value::Struct(handle) => self
                .runtime
                .gc()
                .struct_get_slot(handle, &layout, field.slot as usize)
                .ok_or(VmError::TypeMismatch("struct layout or field mismatch")),
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
        field: FieldRef,
        value: Register,
    ) -> Result<(), VmError> {
        let value = self.current_frame()?.read_register(value)?;
        if !value.is_default_heap_payload() {
            return Err(VmError::TypeMismatch(
                "write_field expects default-storable value",
            ));
        }
        let layout = self
            .current_loaded()?
            .struct_layout(field.structure)
            .ok_or(VmError::TypeMismatch("invalid struct layout"))?;
        match self.current_frame()?.read_register(base)? {
            Value::Struct(handle) => self
                .runtime
                .gc()
                .struct_set_slot(handle, &layout, field.slot as usize, value)
                .map_err(VmError::from),
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
                    .map_err(|error| {
                        if error.kind() == kagari_runtime::RuntimeErrorKind::IndexOutOfBounds {
                            VmError::InvalidIndex(index)
                        } else {
                            VmError::from(error)
                        }
                    })
            }
            Value::Tuple(mut elements) => {
                let Some(slot) = elements.get_mut(index) else {
                    return Err(VmError::InvalidIndex(index));
                };
                *slot = value;
                self.current_frame_mut()?
                    .write_register(base, Value::Tuple(elements))?;
                Ok(())
            }
            _ => Err(VmError::TypeMismatch(
                "write_index expects array or tuple value",
            )),
        }
    }
}

impl Executor<'_> {
    pub(crate) fn standard_enum_operation(
        &self,
        value: Option<Register>,
        ty: &kagari_ir::module::abi::AbiType,
        op: kagari_ir::module::instruction::StandardEnumOp,
    ) -> Result<Value, VmError> {
        use kagari_ir::module::{
            abi::{AbiType, StandardEnumKind},
            instruction::StandardEnumOp,
        };
        use kagari_runtime::value::EnumTag;
        let AbiType::StandardEnum { kind, args } = ty else {
            return Err(VmError::TypeMismatch("standard enum type"));
        };
        let variant = match op {
            StandardEnumOp::Make(v) | StandardEnumOp::Test(v) | StandardEnumOp::Read(v) => v,
        };
        let tag = match (*kind, variant) {
            (StandardEnumKind::Ordering, 0) => EnumTag::OrderingLess,
            (StandardEnumKind::Ordering, 1) => EnumTag::OrderingEqual,
            (StandardEnumKind::Ordering, 2) => EnumTag::OrderingGreater,
            (StandardEnumKind::Option, 0) => EnumTag::OptionSome,
            (StandardEnumKind::Option, 1) => EnumTag::OptionNone,
            (StandardEnumKind::Result, 0) => EnumTag::ResultOk,
            (StandardEnumKind::Result, 1) => EnumTag::ResultErr,
            _ => return Err(VmError::TypeMismatch("standard enum variant")),
        };
        let value = value
            .map(|v| Ok::<_, VmError>(self.current_frame()?.read_register(v)?))
            .transpose()?;
        let result = if matches!(op, StandardEnumOp::Make(_)) {
            Value::Enum(self.runtime.alloc_enum(tag, value.into_iter().collect())?)
        } else {
            let Some(Value::Enum(id)) = value else {
                return Err(VmError::TypeMismatch("standard enum value"));
            };
            let snapshot = self
                .runtime
                .gc()
                .enum_snapshot(id)
                .ok_or(VmError::TypeMismatch("invalid enum handle"))?;
            let payload_count = match (kind, &snapshot.tag) {
                (
                    StandardEnumKind::Ordering,
                    EnumTag::OrderingLess | EnumTag::OrderingEqual | EnumTag::OrderingGreater,
                ) => 0,
                (StandardEnumKind::Option, EnumTag::OptionNone) => 0,
                (StandardEnumKind::Option, EnumTag::OptionSome)
                | (StandardEnumKind::Result, EnumTag::ResultOk | EnumTag::ResultErr) => 1,
                _ => return Err(VmError::TypeMismatch("standard enum family")),
            };
            if snapshot.fields.len() != payload_count {
                return Err(VmError::TypeMismatch("standard enum payload arity"));
            }
            if matches!(op, StandardEnumOp::Test(_)) {
                Value::Bool(snapshot.tag == tag)
            } else {
                if snapshot.tag != tag {
                    return Err(VmError::TypeMismatch("standard enum payload variant"));
                }
                let value = snapshot
                    .fields
                    .first()
                    .cloned()
                    .ok_or(VmError::TypeMismatch("standard enum payload"))?;
                let output = args
                    .get(variant as usize)
                    .ok_or(VmError::TypeMismatch("standard enum payload type"))?
                    .representation();
                if !value.has_representation(output) {
                    return Err(VmError::TypeMismatch(
                        "standard enum payload representation",
                    ));
                }
                value
            }
        };
        Ok(result)
    }
}

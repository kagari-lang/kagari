//! Bytecode instruction wire tags and operands with bounded nested readers.
use crate::{
    artifact::wire::calls::{CallTargetRecord, FieldRefRecord},
    instruction::{
        BinaryOp, BytecodeInstruction, ConstantId, EnumId, InterfaceTableRef, JumpTarget,
        LocalSlot, ModuleSlot, PathId, Register, StructId, UnaryOp,
    },
    program::ModuleRef,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding},
    },
};
use kagari_contract::{
    ids::FunctionRef,
    numeric::{NumericConversion, NumericOperation},
    operations::IterOp,
};
use kagari_types::{declaration::wire::leaf::NominalTyRecord, ty::wire::WireType};
use serde::{Deserialize, Serialize};

/// File-local BytecodeInstruction facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) enum BytecodeInstructionRecord {
    Await {
        dst: Register,
        value: Register,
        future: WireType,
    },
    Convert {
        dst: Register,
        src: Register,
        conversion: NumericConversion,
    },
    Numeric {
        dst: Register,
        operation: NumericOperation,
        lhs: Register,
        rhs: Option<Register>,
    },
    Iter {
        dst: Register,
        value: Option<Register>,
        ty: WireType,
        op: IterOp,
    },
    LoadConst {
        dst: Register,
        constant: ConstantId,
    },
    LoadLocal {
        dst: Register,
        local: LocalSlot,
    },
    LoadModule {
        dst: Register,
        slot: ModuleSlot,
    },
    StoreLocal {
        local: LocalSlot,
        src: Register,
    },
    StoreModule {
        slot: ModuleSlot,
        src: Register,
    },
    Move {
        dst: Register,
        src: Register,
    },
    Unary {
        dst: Register,
        op: UnaryOp,
        operand: Register,
    },
    Binary {
        dst: Register,
        op: BinaryOp,
        lhs: Register,
        rhs: Register,
    },
    Call {
        dst: Option<Register>,
        callee: CallTargetRecord,
        #[serde(deserialize_with = "kagari_contract::decode_limits::operands")]
        args: Vec<Register>,
    },
    MakeTuple {
        dst: Register,
        #[serde(deserialize_with = "kagari_contract::decode_limits::operands")]
        elements: Vec<Register>,
    },
    RangeBound {
        dst: Register,
        value: Register,
        range: WireType,
        bound: WireType,
        upper: bool,
    },
    MakeRange {
        dst: Register,
        start: Option<Register>,
        end: Option<Register>,
        ty: WireType,
    },
    RepeatArray {
        element: WireType,
        dst: Register,
        value: Register,
        count: Register,
    },
    MakeArray {
        element: WireType,
        dst: Register,
        #[serde(deserialize_with = "kagari_contract::decode_limits::operands")]
        elements: Vec<Register>,
    },
    MakeFuture {
        dst: Register,
        function: FunctionRef,
        #[serde(deserialize_with = "kagari_contract::decode_limits::operands")]
        arguments: Vec<Register>,
        future: WireType,
    },
    MakeClosure {
        dst: Register,
        function: FunctionRef,
        #[serde(deserialize_with = "kagari_contract::decode_limits::operands")]
        captures: Vec<Register>,
    },
    MakeCell {
        dst: Register,
        value: Register,
    },
    ReadCell {
        dst: Register,
        cell: Register,
    },
    WriteCell {
        cell: Register,
        value: Register,
    },
    UpcastInterface {
        dst: Register,
        value: Register,
        source: NominalTyRecord,
        target: NominalTyRecord,
    },
    MakeInterface {
        dst: Register,
        value: Register,
        module: ModuleRef,
        implementation: InterfaceTableRef,
        #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
        arguments: Vec<WireType>,
    },
    MakeStruct {
        dst: Register,
        structure: StructId,
        #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
        arguments: Vec<WireType>,
        #[serde(deserialize_with = "kagari_contract::decode_limits::operands")]
        fields: Vec<Register>,
    },
    MakeEnum {
        dst: Register,
        enumeration: EnumId,
        #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
        arguments: Vec<WireType>,
        variant: u32,
        #[serde(deserialize_with = "kagari_contract::decode_limits::operands")]
        fields: Vec<Register>,
    },
    TestEnumVariant {
        dst: Register,
        value: Register,
        enumeration: EnumId,
        #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
        arguments: Vec<WireType>,
        variant: u32,
    },
    ReadEnumPayload {
        dst: Register,
        value: Register,
        enumeration: EnumId,
        #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
        arguments: Vec<WireType>,
        variant: u32,
        index: u32,
    },
    ReadAggregateField {
        dst: Register,
        base: Register,
        field: FieldRefRecord,
    },
    WriteAggregateField {
        base: Register,
        field: FieldRefRecord,
        value: Register,
    },
    ReadAggregateIndex {
        dst: Register,
        base: Register,
        index: Register,
    },
    WriteAggregateIndex {
        base: Register,
        index: Register,
        value: Register,
    },
    ReadPath {
        dst: Register,
        root_or_view: Register,
        path: PathId,
        #[serde(deserialize_with = "kagari_contract::decode_limits::operands")]
        dynamic_args: Vec<Register>,
    },
    SetPath {
        root_or_view: Register,
        path: PathId,
        #[serde(deserialize_with = "kagari_contract::decode_limits::operands")]
        dynamic_args: Vec<Register>,
        value: Register,
    },
    ModifyPath {
        dst: Option<Register>,
        root_or_view: Register,
        path: PathId,
        #[serde(deserialize_with = "kagari_contract::decode_limits::operands")]
        dynamic_args: Vec<Register>,
        op: BinaryOp,
        value: Register,
    },
    MakePathView {
        dst: Register,
        root_or_view: Register,
        path: PathId,
        #[serde(deserialize_with = "kagari_contract::decode_limits::operands")]
        dynamic_args: Vec<Register>,
    },
    Jump {
        target: JumpTarget,
    },
    Branch {
        cond: Register,
        then_target: JumpTarget,
        else_target: JumpTarget,
    },
    Return(Option<Register>),
    Unreachable,
}

impl BytecodeInstructionRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &BytecodeInstruction,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match value {
            BytecodeInstruction::Await { dst, value, future } => Self::Await {
                dst: *dst,
                value: *value,
                future: WireType::encode(future, encoding, cancel)?,
            },
            BytecodeInstruction::Convert {
                dst,
                src,
                conversion,
            } => Self::Convert {
                dst: *dst,
                src: *src,
                conversion: *conversion,
            },
            BytecodeInstruction::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => Self::Numeric {
                dst: *dst,
                operation: *operation,
                lhs: *lhs,
                rhs: *rhs,
            },
            BytecodeInstruction::Iter { dst, value, ty, op } => Self::Iter {
                dst: *dst,
                value: *value,
                ty: WireType::encode(ty, encoding, cancel)?,
                op: *op,
            },
            BytecodeInstruction::LoadConst { dst, constant } => Self::LoadConst {
                dst: *dst,
                constant: *constant,
            },
            BytecodeInstruction::LoadLocal { dst, local } => Self::LoadLocal {
                dst: *dst,
                local: *local,
            },
            BytecodeInstruction::LoadModule { dst, slot } => Self::LoadModule {
                dst: *dst,
                slot: *slot,
            },
            BytecodeInstruction::StoreLocal { local, src } => Self::StoreLocal {
                local: *local,
                src: *src,
            },
            BytecodeInstruction::StoreModule { slot, src } => Self::StoreModule {
                slot: *slot,
                src: *src,
            },
            BytecodeInstruction::Move { dst, src } => Self::Move {
                dst: *dst,
                src: *src,
            },
            BytecodeInstruction::Unary { dst, op, operand } => Self::Unary {
                dst: *dst,
                op: *op,
                operand: *operand,
            },
            BytecodeInstruction::Binary { dst, op, lhs, rhs } => Self::Binary {
                dst: *dst,
                op: *op,
                lhs: *lhs,
                rhs: *rhs,
            },
            BytecodeInstruction::Call { dst, callee, args } => Self::Call {
                dst: *dst,
                callee: CallTargetRecord::encode(callee, encoding, cancel)?,
                args: args.clone(),
            },
            BytecodeInstruction::MakeTuple { dst, elements } => Self::MakeTuple {
                dst: *dst,
                elements: elements.clone(),
            },
            BytecodeInstruction::RangeBound {
                dst,
                value,
                range,
                bound,
                upper,
            } => Self::RangeBound {
                dst: *dst,
                value: *value,
                range: WireType::encode(range, encoding, cancel)?,
                bound: WireType::encode(bound, encoding, cancel)?,
                upper: *upper,
            },
            BytecodeInstruction::MakeRange {
                dst,
                start,
                end,
                ty,
            } => Self::MakeRange {
                dst: *dst,
                start: *start,
                end: *end,
                ty: WireType::encode(ty, encoding, cancel)?,
            },
            BytecodeInstruction::RepeatArray {
                element,
                dst,
                value,
                count,
            } => Self::RepeatArray {
                element: WireType::encode(element, encoding, cancel)?,
                dst: *dst,
                value: *value,
                count: *count,
            },
            BytecodeInstruction::MakeArray {
                element,
                dst,
                elements,
            } => Self::MakeArray {
                element: WireType::encode(element, encoding, cancel)?,
                dst: *dst,
                elements: elements.clone(),
            },
            BytecodeInstruction::MakeFuture {
                dst,
                function,
                arguments,
                future,
            } => Self::MakeFuture {
                dst: *dst,
                function: *function,
                arguments: arguments.clone(),
                future: WireType::encode(future, encoding, cancel)?,
            },
            BytecodeInstruction::MakeClosure {
                dst,
                function,
                captures,
            } => Self::MakeClosure {
                dst: *dst,
                function: *function,
                captures: captures.clone(),
            },
            BytecodeInstruction::MakeCell { dst, value } => Self::MakeCell {
                dst: *dst,
                value: *value,
            },
            BytecodeInstruction::ReadCell { dst, cell } => Self::ReadCell {
                dst: *dst,
                cell: *cell,
            },
            BytecodeInstruction::WriteCell { cell, value } => Self::WriteCell {
                cell: *cell,
                value: *value,
            },
            BytecodeInstruction::UpcastInterface {
                dst,
                value,
                source,
                target,
            } => Self::UpcastInterface {
                dst: *dst,
                value: *value,
                source: NominalTyRecord::encode(source, encoding, cancel)?,
                target: NominalTyRecord::encode(target, encoding, cancel)?,
            },
            BytecodeInstruction::MakeInterface {
                dst,
                value,
                module,
                implementation,
                arguments,
            } => Self::MakeInterface {
                dst: *dst,
                value: *value,
                module: *module,
                implementation: *implementation,
                arguments: arguments
                    .iter()
                    .map(|value| {
                        Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                    })
                    .collect::<Result<_, _>>()?,
            },
            BytecodeInstruction::MakeStruct {
                dst,
                structure,
                arguments,
                fields,
            } => Self::MakeStruct {
                dst: *dst,
                structure: *structure,
                arguments: arguments
                    .iter()
                    .map(|value| {
                        Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                    })
                    .collect::<Result<_, _>>()?,
                fields: fields.clone(),
            },
            BytecodeInstruction::MakeEnum {
                dst,
                enumeration,
                arguments,
                variant,
                fields,
            } => Self::MakeEnum {
                dst: *dst,
                enumeration: *enumeration,
                arguments: arguments
                    .iter()
                    .map(|value| {
                        Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                    })
                    .collect::<Result<_, _>>()?,
                variant: *variant,
                fields: fields.clone(),
            },
            BytecodeInstruction::TestEnumVariant {
                dst,
                value,
                enumeration,
                arguments,
                variant,
            } => Self::TestEnumVariant {
                dst: *dst,
                value: *value,
                enumeration: *enumeration,
                arguments: arguments
                    .iter()
                    .map(|value| {
                        Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                    })
                    .collect::<Result<_, _>>()?,
                variant: *variant,
            },
            BytecodeInstruction::ReadEnumPayload {
                dst,
                value,
                enumeration,
                arguments,
                variant,
                index,
            } => Self::ReadEnumPayload {
                dst: *dst,
                value: *value,
                enumeration: *enumeration,
                arguments: arguments
                    .iter()
                    .map(|value| {
                        Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                    })
                    .collect::<Result<_, _>>()?,
                variant: *variant,
                index: *index,
            },
            BytecodeInstruction::ReadAggregateField { dst, base, field } => {
                Self::ReadAggregateField {
                    dst: *dst,
                    base: *base,
                    field: FieldRefRecord::encode(field, encoding, cancel)?,
                }
            }
            BytecodeInstruction::WriteAggregateField { base, field, value } => {
                Self::WriteAggregateField {
                    base: *base,
                    field: FieldRefRecord::encode(field, encoding, cancel)?,
                    value: *value,
                }
            }
            BytecodeInstruction::ReadAggregateIndex { dst, base, index } => {
                Self::ReadAggregateIndex {
                    dst: *dst,
                    base: *base,
                    index: *index,
                }
            }
            BytecodeInstruction::WriteAggregateIndex { base, index, value } => {
                Self::WriteAggregateIndex {
                    base: *base,
                    index: *index,
                    value: *value,
                }
            }
            BytecodeInstruction::ReadPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => Self::ReadPath {
                dst: *dst,
                root_or_view: *root_or_view,
                path: *path,
                dynamic_args: dynamic_args.clone(),
            },
            BytecodeInstruction::SetPath {
                root_or_view,
                path,
                dynamic_args,
                value,
            } => Self::SetPath {
                root_or_view: *root_or_view,
                path: *path,
                dynamic_args: dynamic_args.clone(),
                value: *value,
            },
            BytecodeInstruction::ModifyPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
                op,
                value,
            } => Self::ModifyPath {
                dst: *dst,
                root_or_view: *root_or_view,
                path: *path,
                dynamic_args: dynamic_args.clone(),
                op: *op,
                value: *value,
            },
            BytecodeInstruction::MakePathView {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => Self::MakePathView {
                dst: *dst,
                root_or_view: *root_or_view,
                path: *path,
                dynamic_args: dynamic_args.clone(),
            },
            BytecodeInstruction::Jump { target } => Self::Jump { target: *target },
            BytecodeInstruction::Branch {
                cond,
                then_target,
                else_target,
            } => Self::Branch {
                cond: *cond,
                then_target: *then_target,
                else_target: *else_target,
            },
            BytecodeInstruction::Return(field0) => Self::Return(*field0),
            BytecodeInstruction::Unreachable => Self::Unreachable,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<BytecodeInstruction, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match self {
            Self::Await { dst, value, future } => BytecodeInstruction::Await {
                dst,
                value,
                future: future.decode(decoding, cancel)?,
            },
            Self::Convert {
                dst,
                src,
                conversion,
            } => BytecodeInstruction::Convert {
                dst,
                src,
                conversion,
            },
            Self::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => BytecodeInstruction::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            },
            Self::Iter { dst, value, ty, op } => BytecodeInstruction::Iter {
                dst,
                value,
                ty: ty.decode(decoding, cancel)?,
                op,
            },
            Self::LoadConst { dst, constant } => BytecodeInstruction::LoadConst { dst, constant },
            Self::LoadLocal { dst, local } => BytecodeInstruction::LoadLocal { dst, local },
            Self::LoadModule { dst, slot } => BytecodeInstruction::LoadModule { dst, slot },
            Self::StoreLocal { local, src } => BytecodeInstruction::StoreLocal { local, src },
            Self::StoreModule { slot, src } => BytecodeInstruction::StoreModule { slot, src },
            Self::Move { dst, src } => BytecodeInstruction::Move { dst, src },
            Self::Unary { dst, op, operand } => BytecodeInstruction::Unary { dst, op, operand },
            Self::Binary { dst, op, lhs, rhs } => BytecodeInstruction::Binary { dst, op, lhs, rhs },
            Self::Call { dst, callee, args } => BytecodeInstruction::Call {
                dst,
                callee: callee.decode(decoding, cancel)?,
                args,
            },
            Self::MakeTuple { dst, elements } => BytecodeInstruction::MakeTuple { dst, elements },
            Self::RangeBound {
                dst,
                value,
                range,
                bound,
                upper,
            } => BytecodeInstruction::RangeBound {
                dst,
                value,
                range: range.decode(decoding, cancel)?,
                bound: bound.decode(decoding, cancel)?,
                upper,
            },
            Self::MakeRange {
                dst,
                start,
                end,
                ty,
            } => BytecodeInstruction::MakeRange {
                dst,
                start,
                end,
                ty: ty.decode(decoding, cancel)?,
            },
            Self::RepeatArray {
                element,
                dst,
                value,
                count,
            } => BytecodeInstruction::RepeatArray {
                element: element.decode(decoding, cancel)?,
                dst,
                value,
                count,
            },
            Self::MakeArray {
                element,
                dst,
                elements,
            } => BytecodeInstruction::MakeArray {
                element: element.decode(decoding, cancel)?,
                dst,
                elements,
            },
            Self::MakeFuture {
                dst,
                function,
                arguments,
                future,
            } => BytecodeInstruction::MakeFuture {
                dst,
                function,
                arguments,
                future: future.decode(decoding, cancel)?,
            },
            Self::MakeClosure {
                dst,
                function,
                captures,
            } => BytecodeInstruction::MakeClosure {
                dst,
                function,
                captures,
            },
            Self::MakeCell { dst, value } => BytecodeInstruction::MakeCell { dst, value },
            Self::ReadCell { dst, cell } => BytecodeInstruction::ReadCell { dst, cell },
            Self::WriteCell { cell, value } => BytecodeInstruction::WriteCell { cell, value },
            Self::UpcastInterface {
                dst,
                value,
                source,
                target,
            } => BytecodeInstruction::UpcastInterface {
                dst,
                value,
                source: source.decode(decoding, cancel)?,
                target: target.decode(decoding, cancel)?,
            },
            Self::MakeInterface {
                dst,
                value,
                module,
                implementation,
                arguments,
            } => BytecodeInstruction::MakeInterface {
                dst,
                value,
                module,
                implementation,
                arguments: arguments
                    .into_iter()
                    .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                    .collect::<Result<_, _>>()?,
            },
            Self::MakeStruct {
                dst,
                structure,
                arguments,
                fields,
            } => BytecodeInstruction::MakeStruct {
                dst,
                structure,
                arguments: arguments
                    .into_iter()
                    .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                    .collect::<Result<_, _>>()?,
                fields,
            },
            Self::MakeEnum {
                dst,
                enumeration,
                arguments,
                variant,
                fields,
            } => BytecodeInstruction::MakeEnum {
                dst,
                enumeration,
                arguments: arguments
                    .into_iter()
                    .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                    .collect::<Result<_, _>>()?,
                variant,
                fields,
            },
            Self::TestEnumVariant {
                dst,
                value,
                enumeration,
                arguments,
                variant,
            } => BytecodeInstruction::TestEnumVariant {
                dst,
                value,
                enumeration,
                arguments: arguments
                    .into_iter()
                    .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                    .collect::<Result<_, _>>()?,
                variant,
            },
            Self::ReadEnumPayload {
                dst,
                value,
                enumeration,
                arguments,
                variant,
                index,
            } => BytecodeInstruction::ReadEnumPayload {
                dst,
                value,
                enumeration,
                arguments: arguments
                    .into_iter()
                    .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                    .collect::<Result<_, _>>()?,
                variant,
                index,
            },
            Self::ReadAggregateField { dst, base, field } => {
                BytecodeInstruction::ReadAggregateField {
                    dst,
                    base,
                    field: field.decode(decoding, cancel)?,
                }
            }
            Self::WriteAggregateField { base, field, value } => {
                BytecodeInstruction::WriteAggregateField {
                    base,
                    field: field.decode(decoding, cancel)?,
                    value,
                }
            }
            Self::ReadAggregateIndex { dst, base, index } => {
                BytecodeInstruction::ReadAggregateIndex { dst, base, index }
            }
            Self::WriteAggregateIndex { base, index, value } => {
                BytecodeInstruction::WriteAggregateIndex { base, index, value }
            }
            Self::ReadPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => BytecodeInstruction::ReadPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
            },
            Self::SetPath {
                root_or_view,
                path,
                dynamic_args,
                value,
            } => BytecodeInstruction::SetPath {
                root_or_view,
                path,
                dynamic_args,
                value,
            },
            Self::ModifyPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
                op,
                value,
            } => BytecodeInstruction::ModifyPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
                op,
                value,
            },
            Self::MakePathView {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => BytecodeInstruction::MakePathView {
                dst,
                root_or_view,
                path,
                dynamic_args,
            },
            Self::Jump { target } => BytecodeInstruction::Jump { target },
            Self::Branch {
                cond,
                then_target,
                else_target,
            } => BytecodeInstruction::Branch {
                cond,
                then_target,
                else_target,
            },
            Self::Return(field0) => BytecodeInstruction::Return(field0),
            Self::Unreachable => BytecodeInstruction::Unreachable,
        })
    }
}

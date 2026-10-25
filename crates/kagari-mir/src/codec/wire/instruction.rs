//! MIR instruction wire tags and operands; decoding grants no execution proof.
use crate::instruction::ConstructionTarget;
use crate::{
    codec::wire::calls::{AggregateFieldRefRecord, CallTargetRecord, PathRefRecord},
    ids::{InstanceId, LocalId, ModuleSlotId},
    instruction::{Constant, Instruction, MirValue, StructFieldInitBuffer, ValueBuffer},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use kagari_contract::callable::{
    construction::wire::ConstructionRequirementRecord, wire::evidence::EvidenceRecord,
};
use kagari_contract::{
    numeric::{NumericConversion, NumericOperation},
    operations::{BinaryOp, IterOp, UnaryOp},
};
use kagari_types::{declaration::wire::leaf::NominalTyRecord, ty::wire::WireType};
use serde::{Deserialize, Serialize};

/// File-local Instruction facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(crate) enum InstructionRecord {
    Await {
        dst: MirValue,
        value: MirValue,
        future: WireType,
    },
    Convert {
        dst: MirValue,
        src: MirValue,
        conversion: NumericConversion,
    },
    Numeric {
        dst: MirValue,
        operation: NumericOperation,
        lhs: MirValue,
        rhs: Option<MirValue>,
    },
    Iter {
        dst: MirValue,
        value: Option<MirValue>,
        ty: WireType,
        op: IterOp,
    },
    LoadConst {
        dst: MirValue,
        constant: Constant,
    },
    LoadLocal {
        dst: MirValue,
        local: LocalId,
    },
    LoadModule {
        dst: MirValue,
        slot: ModuleSlotId,
    },
    StoreLocal {
        local: LocalId,
        src: MirValue,
    },
    StoreModule {
        slot: ModuleSlotId,
        src: MirValue,
    },
    Move {
        dst: MirValue,
        src: MirValue,
    },
    Unary {
        dst: MirValue,
        op: UnaryOp,
        operand: MirValue,
    },
    Binary {
        dst: MirValue,
        op: BinaryOp,
        lhs: MirValue,
        rhs: MirValue,
    },
    Call {
        dst: Option<MirValue>,
        callee: CallTargetRecord,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        args: ValueBuffer,
    },
    MakeTuple {
        dst: MirValue,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        elements: ValueBuffer,
    },
    RangeBound {
        dst: MirValue,
        value: MirValue,
        range: WireType,
        bound: WireType,
        upper: bool,
    },
    MakeRange {
        dst: MirValue,
        start: Option<MirValue>,
        end: Option<MirValue>,
        ty: WireType,
    },
    RepeatArray {
        element: WireType,
        dst: MirValue,
        value: MirValue,
        count: MirValue,
    },
    MakeArray {
        element: WireType,
        dst: MirValue,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        elements: ValueBuffer,
    },
    MakeFuture {
        dst: MirValue,
        function: InstanceId,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        arguments: ValueBuffer,
        future: WireType,
    },
    MakeClosure {
        dst: MirValue,
        function: InstanceId,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        captures: ValueBuffer,
    },
    MakeCell {
        dst: MirValue,
        value: MirValue,
    },
    ReadCell {
        dst: MirValue,
        cell: MirValue,
    },
    WriteCell {
        cell: MirValue,
        value: MirValue,
    },
    UpcastInterface {
        dst: MirValue,
        value: MirValue,
        source: NominalTyRecord,
        target: NominalTyRecord,
    },
    MakeInterface {
        dst: MirValue,
        value: MirValue,
        target: ConstructionTargetRecord,
    },
    MakeStruct {
        dst: MirValue,
        structure: NominalTyRecord,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        fields: StructFieldInitBuffer,
    },
    MakeEnum {
        dst: MirValue,
        enumeration: NominalTyRecord,
        variant: usize,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        fields: ValueBuffer,
    },
    TestEnumVariant {
        dst: MirValue,
        value: MirValue,
        enumeration: NominalTyRecord,
        variant: usize,
    },
    ReadEnumPayload {
        dst: MirValue,
        value: MirValue,
        enumeration: NominalTyRecord,
        variant: usize,
        index: usize,
    },
    ReadAggregateField {
        dst: MirValue,
        base: MirValue,
        field: AggregateFieldRefRecord,
    },
    WriteAggregateField {
        base: MirValue,
        field: AggregateFieldRefRecord,
        value: MirValue,
    },
    ReadAggregateIndex {
        dst: MirValue,
        base: MirValue,
        index: MirValue,
    },
    WriteAggregateIndex {
        base: MirValue,
        index: MirValue,
        value: MirValue,
    },
    ReadPath {
        dst: MirValue,
        root_or_view: MirValue,
        path: PathRefRecord,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        dynamic_args: ValueBuffer,
    },
    SetPath {
        root_or_view: MirValue,
        path: PathRefRecord,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        dynamic_args: ValueBuffer,
        value: MirValue,
    },
    ModifyPath {
        dst: Option<MirValue>,
        root_or_view: MirValue,
        path: PathRefRecord,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        dynamic_args: ValueBuffer,
        op: BinaryOp,
        value: MirValue,
    },
    MakePathView {
        dst: MirValue,
        root_or_view: MirValue,
        path: PathRefRecord,
        #[serde(deserialize_with = "crate::codec::small_operands")]
        dynamic_args: ValueBuffer,
    },
}

impl InstructionRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn encode(
        value: &Instruction,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match value {
            Instruction::Await { dst, value, future } => Self::Await {
                dst: *dst,
                value: *value,
                future: WireType::encode(future, encoding, cancel)?,
            },
            Instruction::Convert {
                dst,
                src,
                conversion,
            } => Self::Convert {
                dst: *dst,
                src: *src,
                conversion: *conversion,
            },
            Instruction::Numeric {
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
            Instruction::Iter { dst, value, ty, op } => Self::Iter {
                dst: *dst,
                value: *value,
                ty: WireType::encode(ty, encoding, cancel)?,
                op: *op,
            },
            Instruction::LoadConst { dst, constant } => Self::LoadConst {
                dst: *dst,
                constant: constant.clone(),
            },
            Instruction::LoadLocal { dst, local } => Self::LoadLocal {
                dst: *dst,
                local: *local,
            },
            Instruction::LoadModule { dst, slot } => Self::LoadModule {
                dst: *dst,
                slot: *slot,
            },
            Instruction::StoreLocal { local, src } => Self::StoreLocal {
                local: *local,
                src: *src,
            },
            Instruction::StoreModule { slot, src } => Self::StoreModule {
                slot: *slot,
                src: *src,
            },
            Instruction::Move { dst, src } => Self::Move {
                dst: *dst,
                src: *src,
            },
            Instruction::Unary { dst, op, operand } => Self::Unary {
                dst: *dst,
                op: *op,
                operand: *operand,
            },
            Instruction::Binary { dst, op, lhs, rhs } => Self::Binary {
                dst: *dst,
                op: *op,
                lhs: *lhs,
                rhs: *rhs,
            },
            Instruction::Call { dst, callee, args } => Self::Call {
                dst: *dst,
                callee: CallTargetRecord::encode(callee, encoding, cancel)?,
                args: args.clone(),
            },
            Instruction::MakeTuple { dst, elements } => Self::MakeTuple {
                dst: *dst,
                elements: elements.clone(),
            },
            Instruction::RangeBound {
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
            Instruction::MakeRange {
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
            Instruction::RepeatArray {
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
            Instruction::MakeArray {
                element,
                dst,
                elements,
            } => Self::MakeArray {
                element: WireType::encode(element, encoding, cancel)?,
                dst: *dst,
                elements: elements.clone(),
            },
            Instruction::MakeFuture {
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
            Instruction::MakeClosure {
                dst,
                function,
                captures,
            } => Self::MakeClosure {
                dst: *dst,
                function: *function,
                captures: captures.clone(),
            },
            Instruction::MakeCell { dst, value } => Self::MakeCell {
                dst: *dst,
                value: *value,
            },
            Instruction::ReadCell { dst, cell } => Self::ReadCell {
                dst: *dst,
                cell: *cell,
            },
            Instruction::WriteCell { cell, value } => Self::WriteCell {
                cell: *cell,
                value: *value,
            },
            Instruction::UpcastInterface {
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
            Instruction::MakeInterface { dst, value, target } => Self::MakeInterface {
                dst: *dst,
                value: *value,
                target: ConstructionTargetRecord::encode(target, encoding, cancel)?,
            },
            Instruction::MakeStruct {
                dst,
                structure,
                fields,
            } => Self::MakeStruct {
                dst: *dst,
                structure: NominalTyRecord::encode(structure, encoding, cancel)?,
                fields: fields.clone(),
            },
            Instruction::MakeEnum {
                dst,
                enumeration,
                variant,
                fields,
            } => Self::MakeEnum {
                dst: *dst,
                enumeration: NominalTyRecord::encode(enumeration, encoding, cancel)?,
                variant: *variant,
                fields: fields.clone(),
            },
            Instruction::TestEnumVariant {
                dst,
                value,
                enumeration,
                variant,
            } => Self::TestEnumVariant {
                dst: *dst,
                value: *value,
                enumeration: NominalTyRecord::encode(enumeration, encoding, cancel)?,
                variant: *variant,
            },
            Instruction::ReadEnumPayload {
                dst,
                value,
                enumeration,
                variant,
                index,
            } => Self::ReadEnumPayload {
                dst: *dst,
                value: *value,
                enumeration: NominalTyRecord::encode(enumeration, encoding, cancel)?,
                variant: *variant,
                index: *index,
            },
            Instruction::ReadAggregateField { dst, base, field } => Self::ReadAggregateField {
                dst: *dst,
                base: *base,
                field: AggregateFieldRefRecord::encode(field, encoding, cancel)?,
            },
            Instruction::WriteAggregateField { base, field, value } => Self::WriteAggregateField {
                base: *base,
                field: AggregateFieldRefRecord::encode(field, encoding, cancel)?,
                value: *value,
            },
            Instruction::ReadAggregateIndex { dst, base, index } => Self::ReadAggregateIndex {
                dst: *dst,
                base: *base,
                index: *index,
            },
            Instruction::WriteAggregateIndex { base, index, value } => Self::WriteAggregateIndex {
                base: *base,
                index: *index,
                value: *value,
            },
            Instruction::ReadPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => Self::ReadPath {
                dst: *dst,
                root_or_view: *root_or_view,
                path: PathRefRecord::encode(path, encoding, cancel)?,
                dynamic_args: dynamic_args.clone(),
            },
            Instruction::SetPath {
                root_or_view,
                path,
                dynamic_args,
                value,
            } => Self::SetPath {
                root_or_view: *root_or_view,
                path: PathRefRecord::encode(path, encoding, cancel)?,
                dynamic_args: dynamic_args.clone(),
                value: *value,
            },
            Instruction::ModifyPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
                op,
                value,
            } => Self::ModifyPath {
                dst: *dst,
                root_or_view: *root_or_view,
                path: PathRefRecord::encode(path, encoding, cancel)?,
                dynamic_args: dynamic_args.clone(),
                op: *op,
                value: *value,
            },
            Instruction::MakePathView {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => Self::MakePathView {
                dst: *dst,
                root_or_view: *root_or_view,
                path: PathRefRecord::encode(path, encoding, cancel)?,
                dynamic_args: dynamic_args.clone(),
            },
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<Instruction, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match self {
            Self::Await { dst, value, future } => Instruction::Await {
                dst,
                value,
                future: future.decode(decoding, cancel)?,
            },
            Self::Convert {
                dst,
                src,
                conversion,
            } => Instruction::Convert {
                dst,
                src,
                conversion,
            },
            Self::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => Instruction::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            },
            Self::Iter { dst, value, ty, op } => Instruction::Iter {
                dst,
                value,
                ty: ty.decode(decoding, cancel)?,
                op,
            },
            Self::LoadConst { dst, constant } => Instruction::LoadConst { dst, constant },
            Self::LoadLocal { dst, local } => Instruction::LoadLocal { dst, local },
            Self::LoadModule { dst, slot } => Instruction::LoadModule { dst, slot },
            Self::StoreLocal { local, src } => Instruction::StoreLocal { local, src },
            Self::StoreModule { slot, src } => Instruction::StoreModule { slot, src },
            Self::Move { dst, src } => Instruction::Move { dst, src },
            Self::Unary { dst, op, operand } => Instruction::Unary { dst, op, operand },
            Self::Binary { dst, op, lhs, rhs } => Instruction::Binary { dst, op, lhs, rhs },
            Self::Call { dst, callee, args } => Instruction::Call {
                dst,
                callee: callee.decode(decoding, cancel)?,
                args,
            },
            Self::MakeTuple { dst, elements } => Instruction::MakeTuple { dst, elements },
            Self::RangeBound {
                dst,
                value,
                range,
                bound,
                upper,
            } => Instruction::RangeBound {
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
            } => Instruction::MakeRange {
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
            } => Instruction::RepeatArray {
                element: element.decode(decoding, cancel)?,
                dst,
                value,
                count,
            },
            Self::MakeArray {
                element,
                dst,
                elements,
            } => Instruction::MakeArray {
                element: element.decode(decoding, cancel)?,
                dst,
                elements,
            },
            Self::MakeFuture {
                dst,
                function,
                arguments,
                future,
            } => Instruction::MakeFuture {
                dst,
                function,
                arguments,
                future: future.decode(decoding, cancel)?,
            },
            Self::MakeClosure {
                dst,
                function,
                captures,
            } => Instruction::MakeClosure {
                dst,
                function,
                captures,
            },
            Self::MakeCell { dst, value } => Instruction::MakeCell { dst, value },
            Self::ReadCell { dst, cell } => Instruction::ReadCell { dst, cell },
            Self::WriteCell { cell, value } => Instruction::WriteCell { cell, value },
            Self::UpcastInterface {
                dst,
                value,
                source,
                target,
            } => Instruction::UpcastInterface {
                dst,
                value,
                source: source.decode(decoding, cancel)?,
                target: target.decode(decoding, cancel)?,
            },
            Self::MakeInterface { dst, value, target } => Instruction::MakeInterface {
                dst,
                value,
                target: target.decode(decoding, cancel)?,
            },
            Self::MakeStruct {
                dst,
                structure,
                fields,
            } => Instruction::MakeStruct {
                dst,
                structure: structure.decode(decoding, cancel)?,
                fields,
            },
            Self::MakeEnum {
                dst,
                enumeration,
                variant,
                fields,
            } => Instruction::MakeEnum {
                dst,
                enumeration: enumeration.decode(decoding, cancel)?,
                variant,
                fields,
            },
            Self::TestEnumVariant {
                dst,
                value,
                enumeration,
                variant,
            } => Instruction::TestEnumVariant {
                dst,
                value,
                enumeration: enumeration.decode(decoding, cancel)?,
                variant,
            },
            Self::ReadEnumPayload {
                dst,
                value,
                enumeration,
                variant,
                index,
            } => Instruction::ReadEnumPayload {
                dst,
                value,
                enumeration: enumeration.decode(decoding, cancel)?,
                variant,
                index,
            },
            Self::ReadAggregateField { dst, base, field } => Instruction::ReadAggregateField {
                dst,
                base,
                field: field.decode(decoding, cancel)?,
            },
            Self::WriteAggregateField { base, field, value } => Instruction::WriteAggregateField {
                base,
                field: field.decode(decoding, cancel)?,
                value,
            },
            Self::ReadAggregateIndex { dst, base, index } => {
                Instruction::ReadAggregateIndex { dst, base, index }
            }
            Self::WriteAggregateIndex { base, index, value } => {
                Instruction::WriteAggregateIndex { base, index, value }
            }
            Self::ReadPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => Instruction::ReadPath {
                dst,
                root_or_view,
                path: path.decode(decoding, cancel)?,
                dynamic_args,
            },
            Self::SetPath {
                root_or_view,
                path,
                dynamic_args,
                value,
            } => Instruction::SetPath {
                root_or_view,
                path: path.decode(decoding, cancel)?,
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
            } => Instruction::ModifyPath {
                dst,
                root_or_view,
                path: path.decode(decoding, cancel)?,
                dynamic_args,
                op,
                value,
            },
            Self::MakePathView {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => Instruction::MakePathView {
                dst,
                root_or_view,
                path: path.decode(decoding, cancel)?,
                dynamic_args,
            },
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) enum ConstructionTargetRecord {
    Declared {
        implementation: PortableDefinitionRef,
        #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
        arguments: Vec<WireType>,
        #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
        operations: Vec<EvidenceRecord>,
        #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
        constructions: Vec<EvidenceRecord>,
    },
    Bound(ConstructionRequirementRecord),
}

impl ConstructionTargetRecord {
    fn encode(
        value: &ConstructionTarget,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match value {
            ConstructionTarget::Declared {
                implementation,
                arguments,
                operations,
                constructions,
            } => Self::Declared {
                implementation: encoding.reference(*implementation)?,
                arguments: arguments
                    .iter()
                    .map(|ty| WireType::encode(ty, encoding, cancel))
                    .collect::<Result<_, _>>()?,
                operations: operations
                    .iter()
                    .map(|value| EvidenceRecord::encode_operation(value, encoding, cancel))
                    .collect::<Result<_, _>>()?,
                constructions: constructions
                    .iter()
                    .map(|value| EvidenceRecord::encode_construction(value, encoding, cancel))
                    .collect::<Result<_, _>>()?,
            },
            ConstructionTarget::Bound(required) => Self::Bound(
                ConstructionRequirementRecord::encode(required, encoding, cancel)?,
            ),
        })
    }

    fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<ConstructionTarget, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match self {
            Self::Declared {
                implementation,
                arguments,
                operations,
                constructions,
            } => ConstructionTarget::Declared {
                implementation: decoding.resolve(implementation)?,
                arguments: arguments
                    .into_iter()
                    .map(|ty| ty.decode(decoding, cancel))
                    .collect::<Result<_, _>>()?,
                operations: operations
                    .into_iter()
                    .map(|value| value.decode_operation(decoding, cancel))
                    .collect::<Result<_, _>>()?,
                constructions: constructions
                    .into_iter()
                    .map(|value| value.decode_construction(decoding, cancel))
                    .collect::<Result<_, _>>()?,
            },
            Self::Bound(required) => ConstructionTarget::Bound(required.decode(decoding, cancel)?),
        })
    }
}

//! Register roles for physical allocation. Locals retain distinct debug storage.
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget, Register};
use kagari_common::identity::table::DefinitionId;
use std::iter;

pub(super) fn reads(instruction: &BytecodeInstruction<DefinitionId>) -> Vec<Register> {
    match instruction {
        BytecodeInstruction::Await { value, .. } => vec![*value],
        BytecodeInstruction::Convert { src, .. } => vec![*src],
        BytecodeInstruction::Numeric { lhs, rhs, .. } => {
            iter::once(*lhs).chain(rhs.iter().copied()).collect()
        }
        BytecodeInstruction::Iter { value, .. } => value.iter().copied().collect(),
        BytecodeInstruction::LoadConst { .. }
        | BytecodeInstruction::LoadLocal { .. }
        | BytecodeInstruction::LoadModule { .. } => vec![],
        BytecodeInstruction::StoreLocal { src, .. }
        | BytecodeInstruction::StoreModule { src, .. }
        | BytecodeInstruction::Move { src, .. } => {
            vec![*src]
        }
        BytecodeInstruction::Unary { operand, .. } => vec![*operand],
        BytecodeInstruction::BeginIteration { collection } => vec![*collection],
        BytecodeInstruction::EndIteration
        | BytecodeInstruction::Jump { .. }
        | BytecodeInstruction::Unreachable => vec![],
        BytecodeInstruction::Branch { cond, .. } => vec![*cond],
        BytecodeInstruction::Return(value) => value.iter().copied().collect(),
        BytecodeInstruction::RangeBound { value, .. } => vec![*value],
        BytecodeInstruction::MakeRange { start, end, .. } => {
            start.iter().chain(end).copied().collect()
        }
        BytecodeInstruction::RepeatArray { value, count, .. } => vec![*value, *count],
        BytecodeInstruction::Binary { lhs, rhs, .. } => vec![*lhs, *rhs],
        BytecodeInstruction::Call { callee, args, .. } => {
            let mut values: Vec<Register> = args.to_vec();
            if let CallTarget::Register(value) = callee {
                values.push(*value);
            }
            if let CallTarget::ClosureRegister {
                register: value, ..
            } = callee
            {
                values.push(*value);
            }
            values
        }
        BytecodeInstruction::MakeTuple { elements, .. }
        | BytecodeInstruction::MakeArray { elements, .. }
        | BytecodeInstruction::MakeEnum {
            fields: elements, ..
        } => elements.to_vec(),
        BytecodeInstruction::MakeClosure { captures, .. } => captures.to_vec(),
        BytecodeInstruction::MakeCell { value, .. } => vec![*value],
        BytecodeInstruction::ReadCell { cell, .. } => vec![*cell],
        BytecodeInstruction::WriteCell { cell, value } => vec![*cell, *value],
        BytecodeInstruction::MakeStruct { fields, .. } => fields.to_vec(),
        BytecodeInstruction::MakeInterface { value, .. }
        | BytecodeInstruction::UpcastInterface { value, .. } => {
            vec![*value]
        }
        BytecodeInstruction::TestEnumVariant { value, .. }
        | BytecodeInstruction::ReadEnumPayload { value, .. } => {
            vec![*value]
        }
        BytecodeInstruction::ReadAggregateField { base, .. } => vec![*base],
        BytecodeInstruction::WriteAggregateField { base, value, .. } => {
            vec![*base, *value]
        }
        BytecodeInstruction::ReadAggregateIndex { base, index, .. } => {
            vec![*base, *index]
        }
        BytecodeInstruction::WriteAggregateIndex {
            base, index, value, ..
        } => vec![*base, *index, *value],
        BytecodeInstruction::ReadPath {
            root_or_view,
            dynamic_args,
            ..
        }
        | BytecodeInstruction::MakePathView {
            root_or_view,
            dynamic_args,
            ..
        } => {
            let mut values: Vec<Register> = dynamic_args.to_vec();
            values.push(*root_or_view);
            values
        }
        BytecodeInstruction::SetPath {
            root_or_view,
            dynamic_args,
            value,
            ..
        }
        | BytecodeInstruction::ModifyPath {
            root_or_view,
            dynamic_args,
            value,
            ..
        } => {
            let mut values: Vec<Register> = dynamic_args.to_vec();
            values.push(*root_or_view);
            values.push(*value);
            values
        }
    }
}

pub(super) fn writes(instruction: &BytecodeInstruction<DefinitionId>) -> Option<Register> {
    match instruction {
        BytecodeInstruction::LoadConst { dst, .. }
        | BytecodeInstruction::LoadLocal { dst, .. }
        | BytecodeInstruction::LoadModule { dst, .. }
        | BytecodeInstruction::Move { dst, .. }
        | BytecodeInstruction::Unary { dst, .. }
        | BytecodeInstruction::Binary { dst, .. }
        | BytecodeInstruction::MakeTuple { dst, .. }
        | BytecodeInstruction::MakeArray { dst, .. }
        | BytecodeInstruction::RepeatArray { dst, .. }
        | BytecodeInstruction::MakeRange { dst, .. }
        | BytecodeInstruction::RangeBound { dst, .. }
        | BytecodeInstruction::MakeClosure { dst, .. }
        | BytecodeInstruction::MakeCell { dst, .. }
        | BytecodeInstruction::ReadCell { dst, .. }
        | BytecodeInstruction::MakeStruct { dst, .. }
        | BytecodeInstruction::Await { dst, .. }
        | BytecodeInstruction::Convert { dst, .. }
        | BytecodeInstruction::Numeric { dst, .. }
        | BytecodeInstruction::Iter { dst, .. }
        | BytecodeInstruction::MakeEnum { dst, .. }
        | BytecodeInstruction::MakeInterface { dst, .. }
        | BytecodeInstruction::UpcastInterface { dst, .. }
        | BytecodeInstruction::TestEnumVariant { dst, .. }
        | BytecodeInstruction::ReadEnumPayload { dst, .. }
        | BytecodeInstruction::ReadAggregateField { dst, .. }
        | BytecodeInstruction::ReadAggregateIndex { dst, .. }
        | BytecodeInstruction::ReadPath { dst, .. }
        | BytecodeInstruction::MakePathView { dst, .. } => Some(*dst),
        BytecodeInstruction::Call { dst, .. } | BytecodeInstruction::ModifyPath { dst, .. } => *dst,
        BytecodeInstruction::StoreLocal { .. }
        | BytecodeInstruction::StoreModule { .. }
        | BytecodeInstruction::WriteAggregateField { .. }
        | BytecodeInstruction::WriteAggregateIndex { .. }
        | BytecodeInstruction::SetPath { .. } => None,
        BytecodeInstruction::WriteCell { .. } => None,
        BytecodeInstruction::BeginIteration { .. }
        | BytecodeInstruction::EndIteration
        | BytecodeInstruction::Return(_)
        | BytecodeInstruction::Jump { .. }
        | BytecodeInstruction::Branch { .. }
        | BytecodeInstruction::Unreachable => None,
    }
}

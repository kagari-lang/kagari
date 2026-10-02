use crate::{
    ids::BlockId,
    instruction::{CallTarget, Instruction, MirValue, Terminator},
};
use smallvec::SmallVec;
use std::iter;

impl Terminator {
    /// CFG targets, including both edges when a branch has the same target.
    pub fn successors(&self) -> SmallVec<[BlockId; 2]> {
        match self {
            Terminator::Jump(target) => smallvec::smallvec![*target],
            Terminator::Branch {
                then_block,
                else_block,
                ..
            } => smallvec::smallvec![*then_block, *else_block],
            Terminator::Return(_) | Terminator::Unreachable => smallvec::smallvec![],
        }
    }

    /// Value read before taking the control-flow edge.
    pub fn input(&self) -> Option<MirValue> {
        match self {
            Self::Return(value) => *value,
            Self::Branch { cond, .. } => Some(*cond),
            _ => None,
        }
    }
}
impl Instruction {
    /// Temporary uses for dataflow analysis; this order is not evaluation order.
    pub fn inputs(&self) -> SmallVec<[MirValue; 4]> {
        match self {
            Instruction::Convert { src, .. } => smallvec::smallvec![*src],
            Instruction::Numeric { lhs, rhs, .. } => {
                iter::once(*lhs).chain(rhs.iter().copied()).collect()
            }
            Instruction::MapResultError {
                original, error, ..
            } => smallvec::smallvec![*original, *error],
            Instruction::Iter { value, .. } | Instruction::StandardEnum { value, .. } => {
                value.iter().copied().collect()
            }
            Instruction::LoadConst { .. }
            | Instruction::LoadLocal { .. }
            | Instruction::LoadModule { .. } => smallvec::smallvec![],
            Instruction::StoreLocal { src, .. }
            | Instruction::StoreModule { src, .. }
            | Instruction::Move { src, .. } => {
                smallvec::smallvec![*src]
            }
            Instruction::Unary { operand, .. } => smallvec::smallvec![*operand],
            Instruction::BeginIteration { collection } => smallvec::smallvec![*collection],
            Instruction::EndIteration => smallvec::smallvec![],
            Instruction::RangeBound { value, .. } => smallvec::smallvec![*value],
            Instruction::MakeRange { start, end, .. } => start.iter().chain(end).copied().collect(),
            Instruction::RepeatArray { value, count, .. } => smallvec::smallvec![*value, *count],
            Instruction::Binary { lhs, rhs, .. } => smallvec::smallvec![*lhs, *rhs],
            Instruction::Call { callee, args, .. } => {
                let mut values = args.clone();
                if let CallTarget::Value(value) = callee {
                    values.push(*value);
                }
                if let CallTarget::Closure { value, .. } = callee {
                    values.push(*value);
                }
                values
            }
            Instruction::MakeTuple { elements, .. }
            | Instruction::MakeArray { elements, .. }
            | Instruction::MakeEnum {
                fields: elements, ..
            } => elements.clone(),
            Instruction::MakeClosure { captures, .. } => captures.clone(),
            Instruction::MakeCell { value, .. } => smallvec::smallvec![*value],
            Instruction::ReadCell { cell, .. } => smallvec::smallvec![*cell],
            Instruction::WriteCell { cell, value } => smallvec::smallvec![*cell, *value],
            Instruction::MakeStruct { fields, .. } => fields.iter().map(|f| f.value).collect(),
            Instruction::MakeInterface { value, .. }
            | Instruction::UpcastInterface { value, .. } => {
                smallvec::smallvec![*value]
            }
            Instruction::TestEnumVariant { value, .. }
            | Instruction::ReadEnumPayload { value, .. } => {
                smallvec::smallvec![*value]
            }
            Instruction::ReadAggregateField { base, .. } => smallvec::smallvec![*base],
            Instruction::WriteAggregateField { base, value, .. } => {
                smallvec::smallvec![*base, *value]
            }
            Instruction::ReadAggregateIndex { base, index, .. } => {
                smallvec::smallvec![*base, *index]
            }
            Instruction::WriteAggregateIndex {
                base, index, value, ..
            } => smallvec::smallvec![*base, *index, *value],
            Instruction::ReadPath {
                root_or_view,
                dynamic_args,
                ..
            }
            | Instruction::MakePathView {
                root_or_view,
                dynamic_args,
                ..
            } => {
                let mut values = dynamic_args.clone();
                values.push(*root_or_view);
                values
            }
            Instruction::SetPath {
                root_or_view,
                dynamic_args,
                value,
                ..
            }
            | Instruction::ModifyPath {
                root_or_view,
                dynamic_args,
                value,
                ..
            } => {
                let mut values = dynamic_args.clone();
                values.push(*root_or_view);
                values.push(*value);
                values
            }
        }
    }

    /// Temporary defined only after this instruction completes successfully.
    pub fn output(&self) -> Option<MirValue> {
        match self {
            Instruction::LoadConst { dst, .. }
            | Instruction::LoadLocal { dst, .. }
            | Instruction::LoadModule { dst, .. }
            | Instruction::Move { dst, .. }
            | Instruction::Unary { dst, .. }
            | Instruction::Binary { dst, .. }
            | Instruction::MakeTuple { dst, .. }
            | Instruction::MakeArray { dst, .. }
            | Instruction::RepeatArray { dst, .. }
            | Instruction::MakeRange { dst, .. }
            | Instruction::RangeBound { dst, .. }
            | Instruction::MakeClosure { dst, .. }
            | Instruction::MakeCell { dst, .. }
            | Instruction::ReadCell { dst, .. }
            | Instruction::MakeStruct { dst, .. }
            | Instruction::Convert { dst, .. }
            | Instruction::Numeric { dst, .. }
            | Instruction::MapResultError { dst, .. }
            | Instruction::Iter { dst, .. }
            | Instruction::StandardEnum { dst, .. }
            | Instruction::MakeEnum { dst, .. }
            | Instruction::MakeInterface { dst, .. }
            | Instruction::UpcastInterface { dst, .. }
            | Instruction::TestEnumVariant { dst, .. }
            | Instruction::ReadEnumPayload { dst, .. }
            | Instruction::ReadAggregateField { dst, .. }
            | Instruction::ReadAggregateIndex { dst, .. }
            | Instruction::ReadPath { dst, .. }
            | Instruction::MakePathView { dst, .. } => Some(*dst),
            Instruction::Call { dst, .. } | Instruction::ModifyPath { dst, .. } => *dst,
            Instruction::StoreLocal { .. }
            | Instruction::StoreModule { .. }
            | Instruction::WriteAggregateField { .. }
            | Instruction::WriteAggregateIndex { .. }
            | Instruction::SetPath { .. } => None,
            Instruction::WriteCell { .. } => None,
            Instruction::BeginIteration { .. } | Instruction::EndIteration => None,
        }
    }
}

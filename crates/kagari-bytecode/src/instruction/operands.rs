//! Explicit register roles shared by verification and physical allocation.
use crate::instruction::{BytecodeInstruction, CallTarget, Slot};
use std::iter;

impl BytecodeInstruction {
    pub fn slot_inputs(&self) -> Vec<Slot> {
        match self {
            BytecodeInstruction::Await { value, .. } => vec![*value],
            BytecodeInstruction::Convert { src, .. } => vec![*src],
            BytecodeInstruction::Numeric { lhs, rhs, .. } => {
                iter::once(*lhs).chain(rhs.iter().copied()).collect()
            }
            BytecodeInstruction::Iter { value, .. } => value.iter().copied().collect(),
            BytecodeInstruction::LoadConst { .. } | BytecodeInstruction::LoadModule { .. } => {
                vec![]
            }
            BytecodeInstruction::StoreModule { src, .. }
            | BytecodeInstruction::Move { src, .. } => {
                vec![*src]
            }
            BytecodeInstruction::Unary { operand, .. } => vec![*operand],
            BytecodeInstruction::Jump { .. } | BytecodeInstruction::Unreachable => vec![],
            BytecodeInstruction::Branch { cond, .. } => vec![*cond],
            BytecodeInstruction::Return(value) => value.iter().copied().collect(),
            BytecodeInstruction::RangeBound { value, .. } => vec![*value],
            BytecodeInstruction::MakeRange { start, end, .. } => {
                start.iter().chain(end).copied().collect()
            }
            BytecodeInstruction::RepeatArray { value, count, .. } => vec![*value, *count],
            BytecodeInstruction::Binary { lhs, rhs, .. } => vec![*lhs, *rhs],
            BytecodeInstruction::BinaryOperands { lhs, rhs, .. }
            | BytecodeInstruction::PredicateBranch { lhs, rhs, .. } => {
                lhs.slot().into_iter().chain(rhs.slot()).collect()
            }
            BytecodeInstruction::Call { callee, args, .. } => {
                let mut values: Vec<Slot> = args.to_vec();
                if let CallTarget::Slot(value) = callee {
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
            BytecodeInstruction::MakeFuture { arguments, .. } => arguments.clone(),
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
                let mut values: Vec<Slot> = dynamic_args.to_vec();
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
                let mut values: Vec<Slot> = dynamic_args.to_vec();
                values.push(*root_or_view);
                values.push(*value);
                values
            }
        }
    }

    pub fn slot_output(&self) -> Option<Slot> {
        match self {
            BytecodeInstruction::LoadConst { dst, .. }
            | BytecodeInstruction::LoadModule { dst, .. }
            | BytecodeInstruction::Move { dst, .. }
            | BytecodeInstruction::Unary { dst, .. }
            | BytecodeInstruction::Binary { dst, .. }
            | BytecodeInstruction::BinaryOperands { dst, .. }
            | BytecodeInstruction::MakeTuple { dst, .. }
            | BytecodeInstruction::MakeArray { dst, .. }
            | BytecodeInstruction::RepeatArray { dst, .. }
            | BytecodeInstruction::MakeRange { dst, .. }
            | BytecodeInstruction::RangeBound { dst, .. }
            | BytecodeInstruction::MakeFuture { dst, .. }
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
            BytecodeInstruction::Call { dst, .. } | BytecodeInstruction::ModifyPath { dst, .. } => {
                *dst
            }
            BytecodeInstruction::StoreModule { .. }
            | BytecodeInstruction::WriteAggregateField { .. }
            | BytecodeInstruction::WriteAggregateIndex { .. }
            | BytecodeInstruction::SetPath { .. } => None,
            BytecodeInstruction::WriteCell { .. } => None,
            BytecodeInstruction::Return(_)
            | BytecodeInstruction::Jump { .. }
            | BytecodeInstruction::Branch { .. }
            | BytecodeInstruction::PredicateBranch { .. }
            | BytecodeInstruction::Unreachable => None,
        }
    }
}

impl BytecodeInstruction {
    /// Rewrite all logical value operands, including destinations, during placement.
    pub fn rewrite_slots(&mut self, mut rewrite: impl FnMut(Slot) -> Slot) {
        match self {
            Self::Await { dst, value, .. } => {
                *dst = rewrite(*dst);
                *value = rewrite(*value);
            }
            Self::Convert { dst, src, .. } => {
                *dst = rewrite(*dst);
                *src = rewrite(*src);
            }
            Self::Numeric { dst, lhs, rhs, .. } => {
                *dst = rewrite(*dst);
                *lhs = rewrite(*lhs);
                if let Some(slot) = rhs {
                    *slot = rewrite(*slot);
                }
            }
            Self::Iter { dst, value, .. } => {
                *dst = rewrite(*dst);
                if let Some(slot) = value {
                    *slot = rewrite(*slot);
                }
            }
            Self::LoadConst { dst, .. } => {
                *dst = rewrite(*dst);
            }
            Self::LoadModule { dst, .. } => {
                *dst = rewrite(*dst);
            }
            Self::StoreModule { src, .. } => {
                *src = rewrite(*src);
            }
            Self::Move { dst, src, .. } => {
                *dst = rewrite(*dst);
                *src = rewrite(*src);
            }
            Self::Unary { dst, operand, .. } => {
                *dst = rewrite(*dst);
                *operand = rewrite(*operand);
            }
            Self::BinaryOperands { dst, lhs, rhs, .. } => {
                *dst = rewrite(*dst);
                lhs.rewrite_slot(&mut rewrite);
                rhs.rewrite_slot(&mut rewrite);
            }
            Self::PredicateBranch { lhs, rhs, .. } => {
                lhs.rewrite_slot(&mut rewrite);
                rhs.rewrite_slot(&mut rewrite);
            }
            Self::Binary { dst, lhs, rhs, .. } => {
                *dst = rewrite(*dst);
                *lhs = rewrite(*lhs);
                *rhs = rewrite(*rhs);
            }
            Self::Call {
                dst, callee, args, ..
            } => {
                if let Some(slot) = dst {
                    *slot = rewrite(*slot);
                }
                match callee {
                    CallTarget::Slot(slot) | CallTarget::ClosureRegister { register: slot, .. } => {
                        *slot = rewrite(*slot)
                    }
                    _ => {}
                }
                for slot in args {
                    *slot = rewrite(*slot);
                }
            }
            Self::MakeTuple { dst, elements, .. } => {
                *dst = rewrite(*dst);
                for slot in elements {
                    *slot = rewrite(*slot);
                }
            }
            Self::RangeBound { dst, value, .. } => {
                *dst = rewrite(*dst);
                *value = rewrite(*value);
            }
            Self::MakeRange {
                dst, start, end, ..
            } => {
                *dst = rewrite(*dst);
                if let Some(slot) = start {
                    *slot = rewrite(*slot);
                }
                if let Some(slot) = end {
                    *slot = rewrite(*slot);
                }
            }
            Self::RepeatArray {
                dst, value, count, ..
            } => {
                *dst = rewrite(*dst);
                *value = rewrite(*value);
                *count = rewrite(*count);
            }
            Self::MakeArray { dst, elements, .. } => {
                *dst = rewrite(*dst);
                for slot in elements {
                    *slot = rewrite(*slot);
                }
            }
            Self::MakeFuture { dst, arguments, .. } => {
                *dst = rewrite(*dst);
                for slot in arguments {
                    *slot = rewrite(*slot);
                }
            }
            Self::MakeClosure { dst, captures, .. } => {
                *dst = rewrite(*dst);
                for slot in captures {
                    *slot = rewrite(*slot);
                }
            }
            Self::MakeCell { dst, value, .. } => {
                *dst = rewrite(*dst);
                *value = rewrite(*value);
            }
            Self::ReadCell { dst, cell, .. } => {
                *dst = rewrite(*dst);
                *cell = rewrite(*cell);
            }
            Self::WriteCell { cell, value, .. } => {
                *cell = rewrite(*cell);
                *value = rewrite(*value);
            }
            Self::UpcastInterface { dst, value, .. } => {
                *dst = rewrite(*dst);
                *value = rewrite(*value);
            }
            Self::MakeInterface { dst, value, .. } => {
                *dst = rewrite(*dst);
                *value = rewrite(*value);
            }
            Self::MakeStruct { dst, fields, .. } => {
                *dst = rewrite(*dst);
                for slot in fields {
                    *slot = rewrite(*slot);
                }
            }
            Self::MakeEnum { dst, fields, .. } => {
                *dst = rewrite(*dst);
                for slot in fields {
                    *slot = rewrite(*slot);
                }
            }
            Self::TestEnumVariant { dst, value, .. } => {
                *dst = rewrite(*dst);
                *value = rewrite(*value);
            }
            Self::ReadEnumPayload { dst, value, .. } => {
                *dst = rewrite(*dst);
                *value = rewrite(*value);
            }
            Self::ReadAggregateField { dst, base, .. } => {
                *dst = rewrite(*dst);
                *base = rewrite(*base);
            }
            Self::WriteAggregateField { base, value, .. } => {
                *base = rewrite(*base);
                *value = rewrite(*value);
            }
            Self::ReadAggregateIndex {
                dst, base, index, ..
            } => {
                *dst = rewrite(*dst);
                *base = rewrite(*base);
                *index = rewrite(*index);
            }
            Self::WriteAggregateIndex {
                base, index, value, ..
            } => {
                *base = rewrite(*base);
                *index = rewrite(*index);
                *value = rewrite(*value);
            }
            Self::ReadPath {
                dst,
                root_or_view,
                dynamic_args,
                ..
            } => {
                *dst = rewrite(*dst);
                *root_or_view = rewrite(*root_or_view);
                for slot in dynamic_args {
                    *slot = rewrite(*slot);
                }
            }
            Self::SetPath {
                root_or_view,
                dynamic_args,
                value,
                ..
            } => {
                *root_or_view = rewrite(*root_or_view);
                for slot in dynamic_args {
                    *slot = rewrite(*slot);
                }
                *value = rewrite(*value);
            }
            Self::ModifyPath {
                dst,
                root_or_view,
                dynamic_args,
                value,
                ..
            } => {
                if let Some(slot) = dst {
                    *slot = rewrite(*slot);
                }
                *root_or_view = rewrite(*root_or_view);
                for slot in dynamic_args {
                    *slot = rewrite(*slot);
                }
                *value = rewrite(*value);
            }
            Self::MakePathView {
                dst,
                root_or_view,
                dynamic_args,
                ..
            } => {
                *dst = rewrite(*dst);
                *root_or_view = rewrite(*root_or_view);
                for slot in dynamic_args {
                    *slot = rewrite(*slot);
                }
            }
            Self::Branch { cond, .. } => {
                *cond = rewrite(*cond);
            }
            Self::Return(Some(slot)) => {
                *slot = rewrite(*slot);
            }
            _ => {}
        }
    }
}

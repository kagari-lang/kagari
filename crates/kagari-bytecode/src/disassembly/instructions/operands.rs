//! Register lists, field selections and explicit operation vocabulary.

use crate::{
    disassembly::{
        presentation::DisplayContext,
        resolution::contracts::{access, types},
    },
    instruction::{BinaryOp, FieldRef, Slot, UnaryOp},
};
use kagari_contract::{
    numeric::NumericOperation,
    operations::{IterOp, StringIterKind},
};
use kagari_types::{integer::IntegerOp, ty::Ty};
use std::io::{self, Write};

pub(super) fn optional_slot(writer: &mut impl Write, value: Option<Slot>) -> io::Result<()> {
    match value {
        Some(value) => write!(writer, "s{}", value.index()),
        None => write!(writer, "none"),
    }
}

pub(super) fn slots(writer: &mut impl Write, values: &[Slot]) -> io::Result<()> {
    write!(writer, "[")?;
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            write!(writer, ", ")?;
        }
        write!(writer, "s{}", value.index())?;
    }
    write!(writer, "]")
}

pub(super) fn field(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    module: usize,
    field: &FieldRef,
) -> io::Result<()> {
    let FieldRef {
        structure,
        arguments,
        slot,
        reference,
    } = field;
    write!(
        writer,
        "{{@{module}:struct#{}, arguments=",
        structure.index()
    )?;
    types(writer, context, arguments)?;
    write!(writer, ", slot={slot}, reference={}}}", access(*reference))
}

pub(super) fn unary(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "neg",
        UnaryOp::Not => "not",
    }
}

pub(super) fn binary(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    op: BinaryOp,
) -> io::Result<()> {
    let name = match op {
        BinaryOp::Numeric(operation) => {
            write!(writer, "numeric.")?;
            return numeric(writer, context, &operation);
        }
        BinaryOp::Add => "add",
        BinaryOp::Sub => "sub",
        BinaryOp::Mul => "mul",
        BinaryOp::Div => "div",
        BinaryOp::Rem => "rem",
        BinaryOp::Eq => "eq",
        BinaryOp::NotEq => "not_eq",
        BinaryOp::IdentityEq => "identity_eq",
        BinaryOp::IdentityNotEq => "identity_not_eq",
        BinaryOp::Lt => "lt",
        BinaryOp::Gt => "gt",
        BinaryOp::Le => "le",
        BinaryOp::Ge => "ge",
    };
    write!(writer, "{name}")
}

pub(super) fn numeric(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    operation: &NumericOperation,
) -> io::Result<()> {
    let NumericOperation { op, input, rhs } = operation;
    let name = match op {
        IntegerOp::BitAnd => "bit_and",
        IntegerOp::BitOr => "bit_or",
        IntegerOp::BitXor => "bit_xor",
        IntegerOp::BitNot => "bit_not",
        IntegerOp::Shl => "shl",
        IntegerOp::Shr => "shr",
        IntegerOp::CheckedAdd => "checked_add",
        IntegerOp::CheckedSub => "checked_sub",
        IntegerOp::CheckedMul => "checked_mul",
        IntegerOp::CheckedDiv => "checked_div",
        IntegerOp::CheckedRem => "checked_rem",
    };
    write!(
        writer,
        "{name}(input={}, rhs=",
        context.ty(&Ty::Builtin(*input))
    )?;
    match rhs {
        Some(rhs) => write!(writer, "{}", context.ty(&Ty::Builtin(*rhs)))?,
        None => write!(writer, "none")?,
    }
    write!(writer, ")")
}

pub(super) fn iter(op: IterOp) -> &'static str {
    match op {
        IterOp::New => "new",
        IterOp::Next => "next",
        IterOp::String(kind) => match kind {
            StringIterKind::Bytes => "string.bytes",
            StringIterKind::CharIndices => "string.char_indices",
            StringIterKind::Split => "string.split",
            StringIterKind::SplitN => "string.split_n",
            StringIterKind::Whitespace => "string.whitespace",
            StringIterKind::Lines => "string.lines",
        },
    }
}

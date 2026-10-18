//! Exact constant values and retained aggregate layout facts.

use crate::{
    disassembly::{
        descriptors::contracts::arguments,
        presentation::{DisplayContext, quoted},
    },
    instruction::ConstantOperand,
};
use kagari_contract::layout::{EnumLayout, EnumVariantLayout, StructFieldLayout, StructLayout};
use kagari_types::visibility::Visibility;
use std::io::{self, Write};

pub(super) fn constant(
    writer: &mut impl Write,
    owner: usize,
    slot: usize,
    value: &ConstantOperand,
) -> io::Result<()> {
    write!(writer, "  @{owner}:const#{slot} ")?;
    match value {
        ConstantOperand::Unit => writeln!(writer, "unit ()"),
        ConstantOperand::Bool(value) => writeln!(writer, "bool {value}"),
        ConstantOperand::I32(value) => writeln!(writer, "i32 {value}"),
        ConstantOperand::I64(value) => writeln!(writer, "i64 {value}"),
        ConstantOperand::U64(value) => writeln!(writer, "u64 {value}"),
        ConstantOperand::F32(value) => {
            writeln!(writer, "f32 {value} bits=0x{:08x}", value.to_bits())
        }
        ConstantOperand::F64(value) => {
            writeln!(writer, "f64 {value} bits=0x{:016x}", value.to_bits())
        }
        ConstantOperand::Str(value) => writeln!(writer, "str {}", quoted(value)),
    }
}

pub(super) fn structure(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    owner: usize,
    slot: usize,
    layout: &StructLayout,
) -> io::Result<()> {
    let StructLayout {
        declaration,
        arguments: types,
        fields,
    } = layout;
    writeln!(
        writer,
        "  @{owner}:struct#{slot} {} arguments={}",
        context.definition(*declaration),
        arguments(context, types)
    )?;
    for (index, field) in fields.iter().enumerate() {
        let StructFieldLayout {
            visibility: access,
            declaration,
            name,
            ty,
            mutable,
        } = field;
        writeln!(
            writer,
            "    field#{index} {} name={} type={} visibility={} mutable={}",
            context.definition(*declaration),
            quoted(name),
            context.ty(ty),
            visibility(*access),
            mutable
        )?;
    }
    Ok(())
}

pub(super) fn enumeration(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    owner: usize,
    slot: usize,
    layout: &EnumLayout,
) -> io::Result<()> {
    let EnumLayout {
        declaration,
        arguments: types,
        variants,
    } = layout;
    writeln!(
        writer,
        "  @{owner}:enum#{slot} {} arguments={}",
        context.definition(*declaration),
        arguments(context, types)
    )?;
    for (index, variant) in variants.iter().enumerate() {
        let EnumVariantLayout {
            reports_failure,
            declaration,
            payload,
        } = variant;
        writeln!(
            writer,
            "    variant#{index} {} reports_failure={reports_failure}",
            context.definition(*declaration)
        )?;
        for (field, ty) in payload.iter().enumerate() {
            writeln!(writer, "      payload#{field} type={}", context.ty(ty))?;
        }
    }
    Ok(())
}

pub(super) fn visibility(value: Visibility) -> &'static str {
    match value {
        Visibility::Private => "private",
        Visibility::PublicSuper => "pub(super)",
        Visibility::Public => "pub",
        Visibility::PublicCrate => "pub(crate)",
    }
}

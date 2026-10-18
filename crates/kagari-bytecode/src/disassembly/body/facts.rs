//! Authoritative entry/slot facts, kept separate from optional debug copies.

use crate::{
    disassembly::presentation::{DisplayContext, physical},
    module::BytecodeFunction,
};
use kagari_abi::representation::ValueType;
use kagari_contract::slots::SemanticSlots;
use kagari_types::ty::Ty;
use std::{
    collections::BTreeMap,
    io::{self, Write},
};

pub(super) fn write_facts(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    function: &BytecodeFunction,
) -> io::Result<()> {
    let metadata = &function.metadata;
    writeln!(
        writer,
        "  locals: {}, registers: {}",
        function.local_count, function.register_count
    )?;
    write!(writer, "  entry:")?;
    if function.parameter_count == 0 {
        write!(writer, " []")?;
    }
    for index in 0..function.parameter_count {
        write!(writer, " arg{index} -> l{index}")?;
    }
    writeln!(writer)?;
    slots(
        writer,
        context,
        "l",
        &metadata.locals,
        &metadata.semantic.locals,
    )?;
    slots(
        writer,
        context,
        "r",
        &metadata.registers,
        &metadata.semantic.registers,
    )?;
    write!(writer, "  roots: locals=[")?;
    for (index, local) in metadata.roots.locals.iter().enumerate() {
        if index != 0 {
            write!(writer, ", ")?;
        }
        write!(writer, "l{}", local.index())?;
    }
    write!(writer, "] registers=[")?;
    for (index, register) in metadata.roots.registers.iter().enumerate() {
        if index != 0 {
            write!(writer, ", ")?;
        }
        write!(writer, "r{}", register.index())?;
    }
    writeln!(writer, "]")?;
    contracts(writer, context, &metadata.semantic)
}

fn slots(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    prefix: &str,
    physical_types: &[ValueType],
    semantic_types: &BTreeMap<usize, Ty>,
) -> io::Result<()> {
    for (index, ty) in physical_types.iter().enumerate() {
        write!(writer, "  {prefix}{index}: physical={}", physical(*ty))?;
        if let Some(ty) = semantic_types.get(&index) {
            write!(writer, " semantic={}", context.ty(ty))?;
        } else {
            write!(writer, " semantic=unknown")?;
        }
        writeln!(writer)?;
    }
    Ok(())
}

fn contracts(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    semantic: &SemanticSlots,
) -> io::Result<()> {
    if let Some(generic) = &semantic.generic {
        writeln!(writer, "  generic:")?;
        for (index, parameter) in generic.parameters.iter().enumerate() {
            writeln!(
                writer,
                "    parameter#{index}: owner={} position={}",
                context.definition(parameter.owner),
                parameter.position
            )?;
        }
        for (index, bound) in generic.bounds.iter().enumerate() {
            write!(
                writer,
                "    bound#{index}: type={} constraints=[",
                context.ty(&bound.ty)
            )?;
            for (index, constraint) in bound.constraints.iter().enumerate() {
                if index != 0 {
                    write!(writer, ", ")?;
                }
                write!(writer, "{}", context.ty(&Ty::Trait(constraint.clone())))?;
            }
            writeln!(writer, "]")?;
        }
    }
    if let Some(adapter) = &semantic.protocol_adapter {
        write!(
            writer,
            "  protocol_adapter: receiver={} interface={} member={} arguments=[",
            context.ty(&adapter.receiver),
            context.ty(&Ty::Trait(adapter.interface.clone())),
            context.definition(adapter.member)
        )?;
        for (index, argument) in adapter.arguments.iter().enumerate() {
            if index != 0 {
                write!(writer, ", ")?;
            }
            write!(writer, "{}", context.ty(argument))?;
        }
        writeln!(writer, "]")?;
    }
    Ok(())
}

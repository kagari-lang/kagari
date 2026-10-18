//! Exhaustive, streaming rendering of verified instruction operands.

mod calls;
mod operands;

#[cfg(test)]
mod tests;

use crate::{
    disassembly::{
        instructions::{
            calls::{function, target},
            operands::{binary, field, iter, numeric, optional_register, registers, unary},
        },
        presentation::DisplayContext,
        resolution::{Resolution, contracts::types},
    },
    instruction::BytecodeInstruction,
};
use kagari_contract::numeric::NumericConversion;
use kagari_types::ty::Ty;
use std::io::{self, Write};

/// Writes one operation and all its operands, without numbering or a final newline.
///
/// The caller supplies a verified program and its retained definition scope. Table
/// operands stay local to their owning module; callee bodies are never traversed.
/// The caller owns buffering and flushing. Every write failure returns immediately.
pub(crate) fn write_instruction(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    resolution: &Resolution<'_>,
    module: usize,
    instruction: &BytecodeInstruction,
) -> io::Result<()> {
    match instruction {
        BytecodeInstruction::Await { dst, value, future } => write!(
            writer,
            "await r{}, value=r{}, future={}",
            dst.index(),
            value.index(),
            context.ty(future)
        ),
        BytecodeInstruction::Convert {
            dst,
            src,
            conversion,
        } => {
            let NumericConversion { source, target } = conversion;
            write!(
                writer,
                "convert r{}, src=r{}, source={}, target={}",
                dst.index(),
                src.index(),
                context.ty(&Ty::Builtin(*source)),
                context.ty(&Ty::Builtin(*target))
            )
        }
        BytecodeInstruction::Numeric {
            dst,
            operation,
            lhs,
            rhs,
        } => {
            write!(writer, "numeric.")?;
            numeric(writer, context, operation)?;
            write!(writer, " dst=r{}, lhs=r{}, rhs=", dst.index(), lhs.index())?;
            optional_register(writer, *rhs)
        }
        BytecodeInstruction::Iter { dst, value, ty, op } => {
            write!(writer, "iter.{} r{}, value=", iter(*op), dst.index())?;
            optional_register(writer, *value)?;
            write!(writer, ", type={}", context.ty(ty))
        }
        BytecodeInstruction::LoadConst { dst, constant } => write!(
            writer,
            "load_const r{}, @{module}:const#{}",
            dst.index(),
            constant.index()
        ),
        BytecodeInstruction::LoadLocal { dst, local } => {
            write!(writer, "load_local r{}, l{}", dst.index(), local.index())
        }
        BytecodeInstruction::LoadModule { dst, slot } => write!(
            writer,
            "load_module r{}, @{module}:slot#{}",
            dst.index(),
            slot.index()
        ),
        BytecodeInstruction::StoreLocal { local, src } => {
            write!(writer, "store_local l{}, r{}", local.index(), src.index())
        }
        BytecodeInstruction::StoreModule { slot, src } => write!(
            writer,
            "store_module @{module}:slot#{}, r{}",
            slot.index(),
            src.index()
        ),
        BytecodeInstruction::Move { dst, src } => {
            write!(writer, "move r{}, r{}", dst.index(), src.index())
        }
        BytecodeInstruction::Unary { dst, op, operand } => write!(
            writer,
            "unary.{} r{}, r{}",
            unary(*op),
            dst.index(),
            operand.index()
        ),
        BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
            write!(writer, "binary.")?;
            binary(writer, context, *op)?;
            write!(
                writer,
                " dst=r{}, lhs=r{}, rhs=r{}",
                dst.index(),
                lhs.index(),
                rhs.index()
            )
        }
        BytecodeInstruction::Call { dst, callee, args } => {
            write!(writer, "call dst=")?;
            optional_register(writer, *dst)?;
            write!(writer, ", target=")?;
            target(writer, context, resolution, module, callee)?;
            write!(writer, ", args=")?;
            registers(writer, args)
        }
        BytecodeInstruction::MakeTuple { dst, elements } => {
            write!(writer, "make_tuple r{}, elements=", dst.index())?;
            registers(writer, elements)
        }
        BytecodeInstruction::RangeBound {
            dst,
            value,
            range,
            bound,
            upper,
        } => write!(
            writer,
            "range_bound r{}, value=r{}, range={}, bound={}, upper={upper}",
            dst.index(),
            value.index(),
            context.ty(range),
            context.ty(bound)
        ),
        BytecodeInstruction::MakeRange {
            dst,
            start,
            end,
            ty,
        } => {
            write!(writer, "make_range r{}, start=", dst.index())?;
            optional_register(writer, *start)?;
            write!(writer, ", end=")?;
            optional_register(writer, *end)?;
            write!(writer, ", type={}", context.ty(ty))
        }
        BytecodeInstruction::RepeatArray {
            element,
            dst,
            value,
            count,
        } => write!(
            writer,
            "repeat_array r{}, value=r{}, count=r{}, element={}",
            dst.index(),
            value.index(),
            count.index(),
            context.ty(element)
        ),
        BytecodeInstruction::MakeArray {
            element,
            dst,
            elements,
        } => {
            write!(
                writer,
                "make_array r{}, element={}, elements=",
                dst.index(),
                context.ty(element)
            )?;
            registers(writer, elements)
        }
        BytecodeInstruction::MakeFuture {
            dst,
            function: callee,
            arguments,
            future,
        } => {
            write!(writer, "make_future r{}, function=", dst.index())?;
            function(writer, context, resolution.program, module, *callee)?;
            write!(writer, ", arguments=")?;
            registers(writer, arguments)?;
            write!(writer, ", future={}", context.ty(future))
        }
        BytecodeInstruction::MakeClosure {
            dst,
            function: callee,
            captures,
        } => {
            write!(writer, "make_closure r{}, function=", dst.index())?;
            function(writer, context, resolution.program, module, *callee)?;
            write!(writer, ", captures=")?;
            registers(writer, captures)
        }
        BytecodeInstruction::MakeCell { dst, value } => write!(
            writer,
            "make_cell r{}, value=r{}",
            dst.index(),
            value.index()
        ),
        BytecodeInstruction::ReadCell { dst, cell } => {
            write!(writer, "read_cell r{}, cell=r{}", dst.index(), cell.index())
        }
        BytecodeInstruction::WriteCell { cell, value } => write!(
            writer,
            "write_cell cell=r{}, value=r{}",
            cell.index(),
            value.index()
        ),
        BytecodeInstruction::UpcastInterface {
            dst,
            value,
            source,
            target,
        } => write!(
            writer,
            "upcast_interface r{}, value=r{}, source={}, target={}",
            dst.index(),
            value.index(),
            context.ty(&Ty::Trait(source.clone())),
            context.ty(&Ty::Trait(target.clone()))
        ),
        BytecodeInstruction::MakeInterface {
            dst,
            value,
            module: owner,
            implementation,
            arguments,
        } => {
            write!(
                writer,
                "make_interface r{}, value=r{}, @{}:interface#{}, arguments=",
                dst.index(),
                value.index(),
                owner.index(),
                implementation.index()
            )?;
            types(writer, context, arguments)
        }
        BytecodeInstruction::MakeStruct {
            dst,
            structure,
            arguments,
            fields,
        } => {
            write!(
                writer,
                "make_struct r{}, @{module}:struct#{}, arguments=",
                dst.index(),
                structure.index()
            )?;
            types(writer, context, arguments)?;
            write!(writer, ", fields=")?;
            registers(writer, fields)
        }
        BytecodeInstruction::MakeEnum {
            dst,
            enumeration,
            arguments,
            variant,
            fields,
        } => {
            write!(
                writer,
                "make_enum r{}, @{module}:enum#{}, arguments=",
                dst.index(),
                enumeration.index()
            )?;
            types(writer, context, arguments)?;
            write!(writer, ", variant={variant}, fields=")?;
            registers(writer, fields)
        }
        BytecodeInstruction::TestEnumVariant {
            dst,
            value,
            enumeration,
            arguments,
            variant,
        } => {
            write!(
                writer,
                "test_enum_variant r{}, value=r{}, @{module}:enum#{}, arguments=",
                dst.index(),
                value.index(),
                enumeration.index()
            )?;
            types(writer, context, arguments)?;
            write!(writer, ", variant={variant}")
        }
        BytecodeInstruction::ReadEnumPayload {
            dst,
            value,
            enumeration,
            arguments,
            variant,
            index,
        } => {
            write!(
                writer,
                "read_enum_payload r{}, value=r{}, @{module}:enum#{}, arguments=",
                dst.index(),
                value.index(),
                enumeration.index()
            )?;
            types(writer, context, arguments)?;
            write!(writer, ", variant={variant}, index={index}")
        }
        BytecodeInstruction::ReadAggregateField {
            dst,
            base,
            field: member,
        } => {
            write!(
                writer,
                "read_aggregate_field r{}, base=r{}, field=",
                dst.index(),
                base.index()
            )?;
            field(writer, context, module, member)
        }
        BytecodeInstruction::WriteAggregateField {
            base,
            field: member,
            value,
        } => {
            write!(
                writer,
                "write_aggregate_field base=r{}, field=",
                base.index()
            )?;
            field(writer, context, module, member)?;
            write!(writer, ", value=r{}", value.index())
        }
        BytecodeInstruction::ReadAggregateIndex { dst, base, index } => write!(
            writer,
            "read_aggregate_index r{}, base=r{}, index=r{}",
            dst.index(),
            base.index(),
            index.index()
        ),
        BytecodeInstruction::WriteAggregateIndex { base, index, value } => write!(
            writer,
            "write_aggregate_index base=r{}, index=r{}, value=r{}",
            base.index(),
            index.index(),
            value.index()
        ),
        BytecodeInstruction::ReadPath {
            dst,
            root_or_view,
            path,
            dynamic_args,
        } => {
            write!(
                writer,
                "read_path r{}, root_or_view=r{}, @{module}:path#{}, dynamic_args=",
                dst.index(),
                root_or_view.index(),
                path.index()
            )?;
            registers(writer, dynamic_args)
        }
        BytecodeInstruction::SetPath {
            root_or_view,
            path,
            dynamic_args,
            value,
        } => {
            write!(
                writer,
                "set_path root_or_view=r{}, @{module}:path#{}, dynamic_args=",
                root_or_view.index(),
                path.index()
            )?;
            registers(writer, dynamic_args)?;
            write!(writer, ", value=r{}", value.index())
        }
        BytecodeInstruction::ModifyPath {
            dst,
            root_or_view,
            path,
            dynamic_args,
            op,
            value,
        } => {
            write!(writer, "modify_path dst=")?;
            optional_register(writer, *dst)?;
            write!(
                writer,
                ", root_or_view=r{}, @{module}:path#{}, dynamic_args=",
                root_or_view.index(),
                path.index()
            )?;
            registers(writer, dynamic_args)?;
            write!(writer, ", op=")?;
            binary(writer, context, *op)?;
            write!(writer, ", value=r{}", value.index())
        }
        BytecodeInstruction::MakePathView {
            dst,
            root_or_view,
            path,
            dynamic_args,
        } => {
            write!(
                writer,
                "make_path_view r{}, root_or_view=r{}, @{module}:path#{}, dynamic_args=",
                dst.index(),
                root_or_view.index(),
                path.index()
            )?;
            registers(writer, dynamic_args)
        }
        BytecodeInstruction::Jump { target } => write!(writer, "jump L{}", target.index()),
        BytecodeInstruction::Branch {
            cond,
            then_target,
            else_target,
        } => write!(
            writer,
            "branch r{}, then=L{}, else=L{}",
            cond.index(),
            then_target.index(),
            else_target.index()
        ),
        BytecodeInstruction::Return(value) => {
            write!(writer, "return ")?;
            optional_register(writer, *value)
        }
        BytecodeInstruction::Unreachable => write!(writer, "unreachable"),
    }
}

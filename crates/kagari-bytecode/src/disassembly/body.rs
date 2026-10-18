//! Stored-order function bodies with execution facts and optional debug annotations.

mod debug;
mod facts;
mod ranges;

#[cfg(test)]
mod tests;

use crate::{
    disassembly::{
        body::debug::DebugIndex, instructions, presentation::DisplayContext,
        resolution::Resolution, selection,
    },
    instruction::BytecodeInstruction,
};
use std::{
    collections::BTreeSet,
    io::{self, Write},
};

/// Writes one verified function without reading source or traversing callees.
///
/// Indices identify stored instructions, not encoded byte offsets. The context
/// must retain this program's verified definition scope and `m:f` must exist.
/// Debug claims never replace execution facts. Stops on the first IO error;
/// buffering and flushing belong to the caller.
pub(crate) fn write_body(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    resolution: &Resolution<'_>,
    m: usize,
    f: usize,
) -> io::Result<()> {
    let module = &resolution.program.modules[m];
    let function = &module.functions[f];
    writeln!(
        writer,
        "{}",
        selection::function_header(context, module, function, m, f)
    )?;
    facts::write_facts(writer, context, function)?;
    let mut debug = DebugIndex::new(&function.metadata.debug);
    debug.write_tables(writer)?;
    let mut labels = BTreeSet::new();
    for instruction in &function.instructions {
        match instruction {
            BytecodeInstruction::Jump { target } => {
                labels.insert(target.index());
            }
            BytecodeInstruction::Branch {
                cond: _,
                then_target,
                else_target,
            } => {
                labels.insert(then_target.index());
                labels.insert(else_target.index());
            }
            _ => {}
        }
    }
    for (index, instruction) in function.instructions.iter().enumerate() {
        if labels.contains(&index) {
            writeln!(writer, "L{index}:")?;
        }
        debug.write_at(writer, index)?;
        write!(writer, "  {index:04}  ")?;
        instructions::write_instruction(writer, context, resolution, m, instruction)?;
        writeln!(writer)?;
    }
    Ok(())
}

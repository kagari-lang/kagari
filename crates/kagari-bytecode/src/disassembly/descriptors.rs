//! Referenced executable tables, borrowed from one verified output operation.

mod collect;
mod contracts;
mod interfaces;
mod native;
mod paths;
mod values;

#[cfg(test)]
mod tests;

use crate::disassembly::{
    presentation::{DisplayContext, physical, quoted},
    resolution::{Resolution, TraitReference, TraitSource},
};
use std::{
    collections::BTreeSet,
    io::{self, Write},
};

/// Declaration order fixes category order within each owning module.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Category {
    Constant,
    Struct,
    Enum,
    Interface,
    PublicTrait,
    TraitContract,
    Path,
    Native,
    Slot,
}

type Reference = (usize, Category, usize);

/// Small artifact-local references, not copies of descriptors or executable bodies.
///
/// Callers supply selected slots from the verified program. Identity-based nested
/// dependencies are resolved only to stored entries; no trait selection or callee
/// instruction traversal takes place here.
#[derive(Default)]
pub(crate) struct References {
    entries: BTreeSet<Reference>,
}

impl References {
    /// Collects descriptors used by selected bodies and their descriptor closure.
    pub(crate) fn collect(resolution: &Resolution<'_>, selected: &[(usize, usize)]) -> Self {
        let mut references = Self::default();
        let mut pending = Vec::new();
        for &(module, function) in selected {
            for instruction in &resolution.program.modules[module].functions[function].instructions
            {
                references.instruction(resolution, module, instruction, &mut pending);
            }
        }
        while let Some(reference) = pending.pop() {
            references.dependencies(resolution, reference, &mut pending);
        }
        references
    }

    fn insert(&mut self, reference: Reference, pending: &mut Vec<Reference>) {
        if self.entries.insert(reference) {
            pending.push(reference);
        }
    }

    /// Writes each referenced entry once in module/category/slot order.
    ///
    /// The program and retained definition context must belong to the same verified
    /// artifact used for collection. Stops on the first writer error; never flushes.
    pub(crate) fn write(
        &self,
        writer: &mut impl Write,
        context: &DisplayContext<'_>,
        resolution: &Resolution<'_>,
    ) -> io::Result<()> {
        let mut current_module = None;

        for &(owner, category, slot) in &self.entries {
            let module = &resolution.program.modules[owner];
            if current_module != Some(owner) {
                writeln!(
                    writer,
                    "descriptors @{owner}: {}",
                    context.module(&module.identity)
                )?;
                current_module = Some(owner);
            }
            match category {
                Category::Constant => {
                    values::constant(writer, owner, slot, &module.constants[slot])?
                }
                Category::Struct => {
                    values::structure(writer, context, owner, slot, &module.structures[slot])?
                }
                Category::Enum => {
                    values::enumeration(writer, context, owner, slot, &module.enumerations[slot])?
                }
                Category::Interface => interfaces::write(writer, context, resolution, owner, slot)?,
                Category::PublicTrait | Category::TraitContract => {
                    let source = if category == Category::PublicTrait {
                        TraitSource::PublicItem
                    } else {
                        TraitSource::Contract
                    };
                    interfaces::declaration(
                        writer,
                        context,
                        resolution,
                        TraitReference {
                            owner,
                            source,
                            slot,
                        },
                    )?;
                }
                Category::Path => {
                    paths::write(writer, owner, slot, &module.paths[slot])?;
                }
                Category::Native => native::write(writer, context, resolution, owner, slot)?,
                Category::Slot => {
                    let record = &module.module_slots[slot];
                    writeln!(
                        writer,
                        "  @{owner}:slot#{slot} name={} physical={} mutable={}",
                        quoted(&record.contract.name),
                        physical(kagari_contract::representation::semantic_representation(
                            &record.contract.ty
                        )),
                        record.contract.mutable
                    )?;
                }
            }
        }
        Ok(())
    }
}

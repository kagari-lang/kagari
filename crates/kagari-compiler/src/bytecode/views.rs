//! Carry preselected interface links into the executable product.
use crate::bytecode::BytecodeLoweringError;
use kagari_bytecode::{module::BytecodeModule, trait_bounds::views};
use kagari_common::cancellation::CancellationToken;

pub(super) fn populate(modules: &mut [BytecodeModule]) -> Result<(), BytecodeLoweringError> {
    let cancel = CancellationToken::default();
    let closure: Vec<_> = modules.iter().collect();
    let mut links = vec![];
    for (owner, module) in modules.iter().enumerate() {
        for (index, table) in module.interface_tables.iter().enumerate() {
            links.push((
                owner,
                index,
                views::links(module, table, &closure, &cancel)
                    .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)?,
            ));
        }
    }
    for (owner, index, links) in links {
        let table = &mut modules[owner].interface_tables[index];
        table.parents = links.parents;
        table.view = links.view;
    }
    Ok(())
}

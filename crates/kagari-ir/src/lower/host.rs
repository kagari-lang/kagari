use super::IrLoweringError;
use kagari_common::{host_interface::HostTypeDeclaration, identity::DefinitionId};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn collect(
    hosts: &kagari_hir::host::HostDeclarations,
    roots: BTreeSet<DefinitionId>,
    functions: &[crate::module::IrFunction],
    cancel: &kagari_common::cancellation::CancellationToken,
) -> Result<Vec<HostTypeDeclaration>, IrLoweringError> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    for instruction in functions
        .iter()
        .flat_map(|f| &f.blocks)
        .flat_map(|b| &b.instructions)
    {
        cancel.check().map_err(|_| IrLoweringError::Cancelled)?;
        if let Some(declaration) = instruction
            .path_reference()
            .and_then(|path| path.declaration.as_ref())
        {
            pending.push(declaration.root.clone());
        }
        if let crate::module::Instruction::Call {
            callee: crate::module::CallTarget::HostFunction(function),
            ..
        } = instruction
        {
            for ty in function
                .params
                .iter()
                .map(|p| &p.ty)
                .chain(std::iter::once(&function.return_type))
            {
                pending.extend(ty.nominal_references().into_iter().cloned());
            }
        }
    }
    let mut declarations = BTreeMap::new();
    while let Some(id) = pending.pop() {
        cancel.check().map_err(|_| IrLoweringError::Cancelled)?;
        if declarations.contains_key(&id) {
            continue;
        }
        let ty = hosts
            .nominal_type(&id)
            .and_then(|id| hosts.type_declaration(id))
            .ok_or(IrLoweringError::MissingBinding(
                "nominal host type declaration",
            ))?;
        for member in ty.value_types() {
            pending.extend(member.nominal_references().into_iter().cloned());
        }
        declarations.insert(id, ty.clone());
    }
    Ok(declarations.into_values().collect())
}

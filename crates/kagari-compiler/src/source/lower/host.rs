use crate::source::lower::MirLoweringError;

use kagari_common::{
    cancellation::CancellationToken,
    host_interface::{HostPathSegmentDeclaration, HostTypeDeclaration},
    identity::DefinitionId,
};
use kagari_hir::host::HostDeclarations;
use kagari_mir::{CallTarget, Instruction, MirFunction};
use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
};

pub(super) fn collect(
    hosts: &HostDeclarations,
    roots: BTreeSet<DefinitionId>,
    functions: &[MirFunction],
    cancel: &CancellationToken,
) -> Result<Vec<HostTypeDeclaration>, MirLoweringError> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    for instruction in functions
        .iter()
        .flat_map(|f| &f.blocks)
        .flat_map(|b| &b.instructions)
    {
        cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
        if let Some(declaration) = instruction
            .path_reference()
            .and_then(|path| path.declaration.as_ref())
        {
            pending.push(declaration.root.clone());
            for segment in &declaration.segments {
                match segment {
                    HostPathSegmentDeclaration::Field(field) => {
                        let mut owner = field.clone();
                        owner.path.pop();
                        pending.push(owner);
                    }
                    HostPathSegmentDeclaration::Index(index) => {
                        for ty in [&index.collection, &index.index, &index.result] {
                            pending.extend(ty.nominal_references().into_iter().cloned());
                        }
                    }
                    HostPathSegmentDeclaration::Virtual(virtual_step) => {
                        pending.extend(
                            virtual_step
                                .result
                                .nominal_references()
                                .into_iter()
                                .cloned(),
                        );
                    }
                }
            }
        }
        if let Instruction::Call {
            callee: CallTarget::Native(import),
            ..
        } = instruction
            && let Some(function) = &import.contract.host
        {
            for ty in function
                .params
                .iter()
                .map(|p| &p.ty)
                .chain(iter::once(&function.return_type))
            {
                pending.extend(ty.nominal_references().into_iter().cloned());
            }
        }
    }
    let mut declarations = BTreeMap::new();
    while let Some(id) = pending.pop() {
        cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
        if declarations.contains_key(&id) {
            continue;
        }
        let ty = hosts
            .nominal_type(&id)
            .and_then(|id| hosts.type_declaration(id))
            .ok_or(MirLoweringError::MissingBinding(
                "nominal host type declaration",
            ))?;
        for member in ty.value_types() {
            pending.extend(member.nominal_references().into_iter().cloned());
        }
        declarations.insert(id, ty.clone());
    }
    Ok(declarations.into_values().collect())
}

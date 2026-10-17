//! Reflect explicit native impl declarations without inferring Rust members.
use super::{Builder, conflict};
use crate::{
    error::RuntimeError,
    metadata::{
        AbiFingerprint, MethodInfo, MethodMetadataId, MethodOrigin, ParameterInfo, TraitInfo,
        TypeId,
    },
};
use kagari_bytecode::artifact::ArtifactFingerprint;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    declaration::{
        NativeDeclaration, TraitDef,
        wire::{declarations::TraitDefRecord, native::NativeDeclarationRecord},
    },
    ty::{NominalTy, Ty, wire::WireType},
};
use std::collections::BTreeSet;

impl Builder<'_> {
    pub(super) fn members(&mut self, owner: DefinitionId, row: TypeId) -> Result<(), RuntimeError> {
        let mut implementations = self
            .catalog
            .implementations
            .entries()
            .filter(|(_, implementation)| implementation.for_type == Ty::Host(owner))
            .collect::<Vec<_>>();
        implementations.sort_by(|(a, _), (b, _)| {
            self.definitions
                .compare(*a, *b)
                .expect("checked implementation identities")
        });
        let mut methods = Vec::new();
        let mut traits = Vec::new();
        let mut next_slot = self.inner.by_id[row.index()].methods.len();
        for (implementation_id, implementation) in implementations {
            if !implementation.generic_params.is_empty() {
                return Err(conflict(
                    "external metadata requires a concrete implementation",
                ));
            }
            let origin = if let Some(interface) = &implementation.trait_type {
                let contract = self
                    .catalog
                    .traits
                    .get_id(interface.declaration)
                    .ok_or_else(|| conflict("external implementation trait is absent"))?;
                let trait_type = self.intern(&Ty::Trait(interface.clone()), 0)?;
                traits.push(TraitInfo {
                    trait_type,
                    name: contract.name.clone(),
                    abi_fingerprint: interface_fingerprint(interface, contract, &self.definitions)?,
                });
                MethodOrigin::Trait(trait_type)
            } else {
                MethodOrigin::Inherent
            };
            for method in &implementation.methods {
                let id = self
                    .definitions
                    .lookup_child(implementation_id, DefinitionKind::Method, &method.name, 0)
                    .ok_or_else(|| conflict("external method identity is absent"))?;
                let declaration = self
                    .catalog
                    .declarations
                    .get_id(id)
                    .ok_or_else(|| conflict("external method declaration is absent"))?;
                let function = &declaration.function;
                if function.host_policy.is_none() || !function.generic_params.is_empty() {
                    return Err(conflict(
                        "external methods require a concrete checked access policy",
                    ));
                }
                let mut params = Vec::new();
                for parameter in &function.params {
                    params.push(ParameterInfo {
                        name: parameter.name.clone(),
                        ty: self.intern(&parameter.ty, 0)?,
                    });
                }
                let return_type = self.intern(&function.return_type, 0)?;
                // Physical reflection slots remain stable when another explicit
                // impl is installed later in this runtime's definition context.
                let metadata_id =
                    *self
                        .inner
                        .native_methods
                        .entry((row, id))
                        .or_insert_with(|| {
                            let id = MethodMetadataId::new(next_slot);
                            next_slot += 1;
                            id
                        });
                methods.push(MethodInfo {
                    id: metadata_id,
                    name: function.name.clone(),
                    params,
                    return_type,
                    origin,
                    abi_fingerprint: method_fingerprint(declaration, &self.definitions)?,
                });
            }
        }
        methods.sort_by_key(|method| method.id.0);
        let info = &mut self.inner.by_id[row.index()];
        info.methods = methods;
        info.traits = traits;
        Ok(())
    }
}

fn method_fingerprint(
    declaration: &NativeDeclaration,
    definitions: &DefinitionTable,
) -> Result<AbiFingerprint, RuntimeError> {
    let cancel = CancellationToken::default();
    let mut references = BTreeSet::new();
    declaration
        .visit_definition_ids(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            &cancel,
        )
        .map_err(conflict)?;
    let encoding = definitions.encode(references).map_err(conflict)?;
    let record =
        NativeDeclarationRecord::encode(declaration, &encoding, &cancel).map_err(conflict)?;
    Ok(AbiFingerprint(
        ArtifactFingerprint::of_serialized(&(record, encoding.table)).0,
    ))
}

fn interface_fingerprint(
    interface: &NominalTy,
    contract: &TraitDef,
    definitions: &DefinitionTable,
) -> Result<AbiFingerprint, RuntimeError> {
    let cancel = CancellationToken::default();
    let mut references = BTreeSet::new();
    let mut visit = |id: &DefinitionId| {
        references.insert(*id);
        Ok(())
    };
    let ty = Ty::Trait(interface.clone());
    ty.visit_definition_ids(&mut visit, &cancel)
        .map_err(conflict)?;
    contract
        .visit_definition_ids(&mut visit, &cancel)
        .map_err(conflict)?;
    let encoding = definitions.encode(references).map_err(conflict)?;
    let contract = TraitDefRecord::encode(contract, &encoding, &cancel).map_err(conflict)?;
    let ty = WireType::encode(&ty, &encoding, &cancel).map_err(conflict)?;
    Ok(AbiFingerprint(
        ArtifactFingerprint::of_serialized(&(ty, contract, encoding.table)).0,
    ))
}

//! Admit only the installed forwarding implementation or the exact source-owned blanket.
use super::{NativeRegistry, dependencies::implementation_matches};
use crate::{
    error::RuntimeError, module::admission::DefinitionAdmission, native::binding::NativeResult,
};
use kagari_bytecode::module::BytecodeModule;
use kagari_common::identity::{
    DefinitionKind,
    table::{DefinitionId, DefinitionTable},
};
use kagari_contract::types::{InterfaceTable, PublicItem, named::NamedContract};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::conversion::{is_library_forwarding_trait, library_forwarding_header_valid},
    language::Protocol,
    ty::Ty,
};
use std::collections::HashSet;

/// Bytecode verification ties the member signature to the trait and the table to
/// its named header. Additionally require the library domain and a direct script body.
pub(super) fn source_member(
    table: &InterfaceTable,
    defs: &DefinitionTable,
) -> Option<DefinitionId> {
    let Ty::Trait(interface) = &table.trait_type else {
        return None;
    };
    let [method] = table.methods.as_slice() else {
        return None;
    };
    let name = if interface.associated_types.is_empty() {
        "into"
    } else {
        "try_into"
    };
    if table.bridge.is_some()
        || !table.associated_consts.is_empty()
        || !table.associated_type_families.is_empty()
        || !table.method_origins.is_empty()
        || method.name != name
        || !matches!(method.params.as_slice(), [receiver] if receiver.name == "self" && !receiver.mutable && receiver.ty == table.for_type)
        || method.implementation != CallableImplementation::Script
        || method.host_policy.is_some()
        || method.method_policy != MethodPolicy::default()
        || !method.bounds.is_empty()
        || !method.generic_params.is_empty()
        || !library_forwarding_header_valid(
            defs,
            table.declaration,
            &table.generic_params,
            &table.for_type,
            interface,
            &table.bounds,
        )
    {
        return None;
    }
    defs.lookup_child(table.declaration, DefinitionKind::Method, name, 0)
}

impl NativeRegistry {
    pub(super) fn validate_forwarding(
        &self,
        module: &BytecodeModule,
        admission: &DefinitionAdmission,
    ) -> NativeResult<()> {
        let invalid = || {
            RuntimeError::module_validation(
                "conversion blanket differs from its installed implementation",
            )
        };
        let defs = admission.source();
        let installed = admission.target();
        let source_owner = !self.catalog.contains_module(&Protocol::From.module());
        let mut tables = HashSet::new();
        let mut interfaces = HashSet::new();
        for item in &module.public_items {
            let PublicItem::InterfaceTable(table) = item else {
                continue;
            };
            let Ty::Trait(interface) = &table.trait_type else {
                continue;
            };
            if !is_library_forwarding_trait(defs, interface.declaration) {
                continue;
            }
            if !tables.insert(table.declaration) || !interfaces.insert(interface.declaration) {
                return Err(invalid());
            }
            if source_owner {
                if module.identity != Protocol::From.module()
                    || source_member(table, defs).is_none()
                {
                    return Err(invalid());
                }
                continue;
            }
            let table = admission.runtime(table.as_ref(), &Default::default())?;
            let expected = self
                .catalog
                .implementations
                .get_id(table.declaration)
                .ok_or_else(invalid)?;
            let [method] = expected.methods.as_slice() else {
                return Err(invalid());
            };
            let member = installed
                .lookup_child(table.declaration, DefinitionKind::Method, &method.name, 0)
                .ok_or_else(invalid)?;
            let interface = admission.runtime_id(interface.declaration)?;
            if !implementation_matches(&table.declaration, expected, &table, installed)
                || !self.owned_traits.contains(&interface)
                || table.methods.len() != 1
                || !table.method_origins.is_empty()
                || method.implementation != CallableImplementation::Script
                || self.scripts.get_id(member).is_none()
            {
                return Err(invalid());
            }
        }
        // Header-only declarations cannot authorize a second domain. Linked verification
        // already requires a corresponding exact named header for every non-bridge table.
        for record in &module.named_declarations {
            if let NamedContract::Implementation(header) = &record.contract
                && let Some(Ty::Trait(interface)) = &header.trait_type
                && is_library_forwarding_trait(defs, interface.declaration)
                && !tables.contains(&record.declaration)
            {
                return Err(invalid());
            }
        }
        if module.identity == Protocol::From.module() {
            if source_owner {
                return (interfaces.len() == 2).then_some(()).ok_or_else(invalid);
            }
            for (id, contract) in self
                .catalog
                .traits
                .entries()
                .filter(|(id, _)| is_library_forwarding_trait(installed, *id))
            {
                let public = module.public_items.iter().any(|item| {
                    matches!(item, PublicItem::Trait(actual) if actual.name == contract.name)
                });
                if !public
                    || !admission
                        .product_id(id)
                        .is_some_and(|product| interfaces.contains(&product))
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Runtime,
        interpreter_tests::common::{compile_test_bytecode, round_trip_bytecode},
        module::entry::ProgramEntry,
        test_support::bytecode::load_bytecode,
    };

    use kagari_bytecode::{
        instruction::{BytecodeInstruction, CallTarget},
        program::updates::{self, EvidenceInputs},
    };
    use kagari_common::identity::metadata::DefinitionMetadata;
    use kagari_types::language::Protocol;

    #[test]
    fn authored_blanket_rejects_a_linked_call_to_a_same_signature_wrapper() {
        let input = round_trip_bytecode(compile_test_bytecode(include_str!(
            "../../../tests/fixtures/native/installed-conversion.kgr"
        )));
        let mut runtime = Runtime::default();
        let loaded = load_bytecode(&mut runtime, "forward", input.clone()).unwrap();
        let result: i64 = runtime
            .execute_typed(&loaded, ProgramEntry::Main, (), &Default::default())
            .unwrap();
        assert_eq!(result, 42);
        // Redirect the closed i32 -> Token call, not its signature. The wrapper
        // even calls the same implementation, but it is not the selected member.
        let (definitions, mut forged) = input.into_parts();
        let root = forged.root;
        let functions = &forged.modules[root.index()].functions;
        let target = |name: &str| functions.iter().find(|f| f.name == name).unwrap().id;
        let (from, wrapper) = (target("from"), target("wrapper"));
        let convert = forged
            .modules
            .iter_mut()
            .find(|m| m.identity == Protocol::From.module())
            .unwrap();
        let call = convert
            .functions
            .iter_mut()
            .flat_map(|f| &mut f.instructions)
            .find_map(|insn| match insn {
                BytecodeInstruction::Call {
                    callee: CallTarget::ModuleFunction { module, function },
                    ..
                } if *module == root && *function == from => Some(function),
                _ => None,
            })
            .expect("concrete blanket calls the selected From implementation");
        *call = wrapper;
        // Present a self-consistent bytecode product; stale update evidence must
        // not stand in for authenticating the qualified forwarding operation.
        let evidence = forged.update_evidence;
        forged.update_evidence = updates::build(
            &forged.modules,
            EvidenceInputs {
                sources: evidence.sources,
                initializers: evidence.initializers,
                selections: evidence.selections,
                classifications: evidence
                    .bodies
                    .into_iter()
                    .map(|body| (body.requirement.origin, body.classification))
                    .collect(),
            },
            &definitions,
            &Default::default(),
        )
        .unwrap();
        let forged = DefinitionMetadata::checked(definitions, forged, &Default::default()).unwrap();
        let error = load_bytecode(&mut Runtime::default(), "forged", forged).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("differs from the required qualified forward call"),
            "{error}"
        );
    }
}

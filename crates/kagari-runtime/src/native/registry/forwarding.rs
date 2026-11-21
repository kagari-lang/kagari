//! A portable forwarding table is evidence only for the blanket actually installed.
use super::{NativeRegistry, dependencies::implementation_matches};
use crate::{
    error::RuntimeError, module::admission::DefinitionAdmission, native::binding::NativeResult,
};
use kagari_bytecode::module::BytecodeModule;
use kagari_common::identity::DefinitionKind;
use kagari_contract::types::{PublicItem, named::NamedContract};
use kagari_types::{
    callable::CallableImplementation, declaration::conversion::is_library_forwarding_trait,
    language::Protocol, ty::Ty,
};
use std::collections::HashSet;

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
                || !interfaces.insert(interface)
                || table.methods.len() != 1
                || !table.method_origins.is_empty()
                || method.implementation != CallableImplementation::Script
                || self.scripts.get_id(member).is_none()
                || !tables.insert(table.declaration)
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
                && !tables.contains(&admission.runtime_id(record.declaration)?)
            {
                return Err(invalid());
            }
        }
        if module.identity == Protocol::From.module() {
            for (id, contract) in self
                .catalog
                .traits
                .entries()
                .filter(|(id, _)| is_library_forwarding_trait(installed, *id))
            {
                let public = module.public_items.iter().any(|item| {
                    matches!(item, PublicItem::Trait(actual) if actual.name == contract.name)
                });
                if !public || !interfaces.contains(&id) {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}

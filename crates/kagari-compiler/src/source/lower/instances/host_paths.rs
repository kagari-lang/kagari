//! Bind checked field identities to the admitted provider's executable path declarations.

use super::InstancePlanner;
use crate::source::lower::{MirLoweringError, host};
use kagari_common::identity::table::DefinitionId;
use kagari_hir::types::{TypeId, semantic::raise_type};
use kagari_types::declaration::{
    host::PathAccess,
    path::{HostPathDeclaration, HostPathSegmentDeclaration},
};

impl InstancePlanner<'_> {
    pub(in crate::source::lower) fn bind_host_fields(
        &self,
        owner: DefinitionId,
        fields: &[(DefinitionId, TypeId)],
        write: bool,
    ) -> Result<(HostPathDeclaration, u64), MirLoweringError> {
        let missing = || MirLoweringError::MissingBinding("registered Host root");
        let module = self
            .modules
            .get(&self.module_handle(owner))
            .ok_or_else(missing)?;
        let mut selected = None;
        for path in module.registered_host_paths() {
            self.check()?;
            if path.root != owner || path.segments.len() != fields.len() {
                continue;
            }
            let same_fields = path.segments.iter().zip(fields).all(|(segment, (field, _))| matches!(segment, HostPathSegmentDeclaration::Field(id) if id == field));
            if same_fields && selected.replace(path).is_some() {
                return Err(MirLoweringError::MissingBinding(
                    "ambiguous registered Host field path",
                ));
            }
        }
        let declaration = selected.ok_or(MirLoweringError::MissingBinding(
            "no registered path for checked Host fields",
        ))?;
        let definitions = self.definitions();
        // Every step is a field: its type is reachable from the root schema, even across modules.
        let schemas = host::schema_closure(
            self.modules,
            vec![owner],
            &definitions,
            &self.options.cancel,
        )?;
        let contract = declaration.contract(&schemas, &definitions)?;
        if contract
            .segments
            .iter()
            .zip(fields)
            .any(|(segment, (_, ty))| raise_type(&segment.result) != *ty)
        {
            return Err(MirLoweringError::MissingBinding(
                "registered Host field type differs from checked selection",
            ));
        }
        if write && declaration.access != PathAccess::ReadWrite {
            return Err(MirLoweringError::MissingBinding(
                "registered Host path is not writable",
            ));
        }
        Ok((declaration.clone(), contract.fingerprint(&definitions)?))
    }
}

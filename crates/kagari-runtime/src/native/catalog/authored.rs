//! Public declaration evidence from ordinary verified bytecode, not native registrations.
use crate::{
    error::RuntimeError,
    module::{VerifiedProgram, admission::DefinitionAdmission},
    native::catalog::{DeclarationCatalog, ownership::insert_contract, projection},
};
use kagari_bytecode::{
    module::BytecodeModule,
    program::verified::{VerifiedBytecodeProgram, VerifiedProductIdentity},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::types::{PublicItem, type_contract};
use kagari_types::language::binding::{future_declaration, option_declaration, result_declaration};
use std::{
    collections::{BTreeSet, HashSet},
    sync::Arc,
};

impl DeclarationCatalog {
    /// Project selected modules' public nominal contracts and Script trait impl ABIs for authoring.
    ///
    /// Use a verified program (or `VerifiedArtifact::bytecode()`), not editable ABI records.
    /// Selection is by package and lexical module path, not export spelling. Private items,
    /// free functions, bridges, host capabilities and re-exports never become providers.
    /// Dependencies retain their identities; select their owners too if needed for sealing.
    /// Reject missing, repeated or conflicting owners and public type/trait contracts that
    /// require native authority. No runtime, callback, Rust mapping or storage is installed.
    pub fn with_verified_modules<'a>(
        &self,
        program: &VerifiedBytecodeProgram,
        owners: impl IntoIterator<Item = &'a ModuleIdentity>,
    ) -> Result<Self, RuntimeError> {
        let mut pending = BTreeSet::new();
        for owner in owners {
            if !pending.insert(owner) {
                return Err(conflict(format!("repeated verified module: {owner}")));
            }
        }
        let public = projection::public_nominals(&program.program().modules, program.definitions());
        let mut projection = Self::in_context(self.types.context().clone());
        for module in &program.program().modules {
            if !pending.remove(&module.identity) {
                continue;
            }
            projection.project_public(module, program.definitions(), &public)?;
            Arc::make_mut(&mut projection.module_owners).insert(module.identity.clone());
            Arc::make_mut(&mut projection.authored)
                .insert(module.identity.clone(), program.product_identity());
        }
        if let Some(missing) = pending.first() {
            return Err(conflict(format!("missing verified module: {missing}")));
        }
        let mut combined = self.clone();
        combined.merge(&projection)?;
        Ok(combined)
    }

    /// Add core value and Future schemas used by conversions and async execution.
    /// Linking has checked the reached owners. These partial records belong to
    /// this publication, never to the registry of installed Native providers.
    pub(crate) fn for_conversions(
        &self,
        program: &VerifiedProgram,
        admission: &DefinitionAdmission,
        cancel: &CancellationToken,
    ) -> Result<Self, RuntimeError> {
        let mut catalog = self.clone();
        for path in [
            option_declaration(),
            result_declaration(),
            future_declaration(),
        ] {
            if self.contains_module(&path.module) {
                continue;
            }
            let Some(module) = program.modules().iter().find(|m| m.identity == path.module) else {
                continue;
            };
            let defs = program.definitions();
            let id = defs
                .lookup(&path)
                .ok_or_else(|| conflict("missing admitted core type"))?;
            let ty = type_contract(&module.identity, &module.public_items, &id, defs)
                .ok_or_else(|| conflict("missing admitted public core type"))?;
            catalog.insert_type(admission.runtime_id(id)?, admission.runtime(ty, cancel)?)?;
            Arc::make_mut(&mut catalog.authored).insert(path.module, program.product_identity());
        }
        Ok(catalog)
    }

    fn project_public(
        &mut self,
        module: &BytecodeModule,
        definitions: &DefinitionTable,
        public: &HashSet<DefinitionId>,
    ) -> Result<(), RuntimeError> {
        let context = self.types.context().clone();
        let root = context.intern_root(&module.identity).map_err(conflict)?;
        let cancel = CancellationToken::default();
        let source_root = definitions
            .lookup_root(&module.identity)
            .ok_or_else(|| conflict("missing verified module root"))?;
        let mut items = Vec::new();
        for item in &module.public_items {
            match item {
                PublicItem::InterfaceTable(table)
                    if projection::source_impl(table, public, definitions).map_err(conflict)? => {}
                _ if projection::nominal(item, source_root, definitions).is_some() => {}
                PublicItem::Type(_) | PublicItem::Trait(_) => {
                    return Err(conflict(format!(
                        "verified public contract requires native authority: {}::{}",
                        module.identity,
                        item.name()
                    )));
                }
                _ => continue,
            }
            items.push(item.clone());
        }
        let items = DefinitionMetadata::checked(definitions.clone(), items, &cancel)
            .and_then(|items| items.import_into(&context, &cancel))
            .map_err(conflict)?;
        for item in items.into_records() {
            match item {
                PublicItem::Type(ty) => {
                    let id = context
                        .intern_child(root, ty.kind.definition_kind(), &ty.name, 0)
                        .map_err(conflict)?;
                    self.insert_type(id, ty)?;
                }
                PublicItem::Trait(tr) => {
                    let id = context
                        .intern_child(root, DefinitionKind::Trait, &tr.name, 0)
                        .map_err(conflict)?;
                    self.insert(id, tr)?;
                }
                PublicItem::InterfaceTable(table) => {
                    insert_contract(
                        &mut self.source_impls,
                        table.declaration,
                        *table,
                        "conflicting authored implementation contracts",
                    )?;
                }
                _ => unreachable!("selected only authoring contracts"),
            }
        }
        Ok(())
    }

    /// The verified product supplying this owner or requirement. This identifies the
    /// authoring input, not a signature of authorship or permission to call native code.
    pub fn authored_origin(&self, owner: &ModuleIdentity) -> Option<VerifiedProductIdentity> {
        self.authored.get(owner).copied()
    }

    /// Extract just the authored part of this view, retaining scoped contract equality
    /// and origins. Partial dependency records never become full module providers.
    pub fn authored_contracts(&self) -> Self {
        let mut result = Self::in_context(self.types.context().clone());
        result.authored = self.authored.clone();
        result.source_impls = self.source_impls.clone();
        if !self.authored.is_empty() {
            result.types = self.types.clone();
            Arc::make_mut(&mut result.types)
                .retain(|path, _| self.authored.contains_key(&path.module));
            result.traits = self.traits.clone();
            Arc::make_mut(&mut result.traits)
                .retain(|path, _| self.authored.contains_key(&path.module));
            result.module_owners = Arc::new(
                self.module_owners
                    .iter()
                    .filter(|owner| self.authored.contains_key(*owner))
                    .cloned()
                    .collect(),
            );
        }
        result
    }

    pub(crate) fn nonempty_authored_contracts(&self) -> Option<Self> {
        (!self.authored.is_empty()).then(|| self.authored_contracts())
    }

    pub(crate) fn authored_owners(&self) -> impl Iterator<Item = &ModuleIdentity> {
        self.authored.keys()
    }

    pub(super) fn check_authored_origins(&self, other: &Self) -> Result<(), RuntimeError> {
        for (owner, origin) in self.authored.iter() {
            if other
                .authored
                .get(owner)
                .is_some_and(|other| other != origin)
                || (other.contains_module(owner) && !other.authored.contains_key(owner))
            {
                return Err(conflict(format!(
                    "conflicting verified/native module origin: {owner}"
                )));
            }
        }
        if !self.authored.is_empty() {
            // Even a partial native dependency catalog cannot add executable authority to a
            // verified owner. Source implementation tables are a distinct proof input.
            let definitions = other.definitions();
            for id in other
                .implementations
                .ids()
                .chain(other.declarations.ids())
                .chain(other.paths.ids())
            {
                if self
                    .authored
                    .contains_key(definitions.resolve(id).map_err(conflict)?.module())
                {
                    return Err(conflict(
                        "native implementation or binding in verified module",
                    ));
                }
            }
            // A reference in a native catalog can agree with a public item, but cannot
            // add a new verified declaration merely by interning a sibling path.
            for (path, _) in other.traits.iter() {
                if self.authored.contains_key(&path.module)
                    && !other.authored.contains_key(&path.module)
                    && self.traits.get(&path).is_none()
                {
                    return Err(conflict("unverified trait in verified module"));
                }
            }
            for (path, _) in other.types.iter() {
                if self.authored.contains_key(&path.module)
                    && !other.authored.contains_key(&path.module)
                    && self.types.get(&path).is_none()
                {
                    return Err(conflict("unverified type in verified module"));
                }
            }
        }
        Ok(())
    }

    pub(super) fn is_authored_definition(&self, id: DefinitionId) -> Result<bool, RuntimeError> {
        Ok(!self.authored.is_empty()
            && self
                .authored
                .contains_key(self.definitions().resolve(id).map_err(conflict)?.module()))
    }

    pub(super) fn retain_authored_origin(
        &mut self,
        source: &Self,
        id: DefinitionId,
    ) -> Result<(), RuntimeError> {
        if !source.authored.is_empty() {
            let definitions = source.definitions();
            let owner = definitions.resolve(id).map_err(conflict)?.module();
            if let Some(origin) = source.authored_origin(owner) {
                Arc::make_mut(&mut self.authored).insert(owner.clone(), origin);
            }
        }
        Ok(())
    }

    pub(super) fn origins_satisfied_by(&self, supplied: &Self) -> Result<bool, RuntimeError> {
        if !self
            .authored
            .keys()
            .all(|owner| supplied.contains_module(owner) && supplied.authored.contains_key(owner))
        {
            return Ok(false);
        }
        // A separately compiled program can satisfy the same exact public contract.
        // Conversely, equal ABI from the wrong kind of provider is not sufficient.
        if !supplied.authored.is_empty() {
            if self.module_owners.iter().any(|owner| {
                supplied.authored.contains_key(owner) && !self.authored.contains_key(owner)
            }) {
                return Ok(false);
            }
            let definitions = self.definitions();
            for id in self.types.ids().chain(self.traits.ids()) {
                let owner = definitions.resolve(id).map_err(conflict)?.module();
                if supplied.authored.contains_key(owner) && !self.authored.contains_key(owner) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}

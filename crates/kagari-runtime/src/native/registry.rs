//! Validate registrations and prepare concrete entries/callables before execution.

mod dependencies;
#[cfg(test)]
mod tests;
use crate::{
    error::RuntimeError,
    host::HostPathAdapter,
    metadata::native::MetadataSnapshot,
    module::VerifiedProgram,
    native::{
        binding::{LinkedNativeFunction, NativeBinding, NativeResult},
        catalog::{DeclarationCatalog, import::CatalogImports},
        context::{CallableOwner, LinkedCallable, LinkedOperation},
        result::LinkedResultAdapter,
        storage::NativeStorage,
    },
};
use kagari_bytecode::{
    instruction::NativeImportId,
    module::{BytecodeModule, CallableTarget},
    program::{ModuleRef, native::NativeApplicationOccurrence},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        map::DefinitionMap,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    callable::witness::OperationWitness,
    native_import::callables::{NativeCallableApplication, NativeCallableOrigin},
    standard::RuntimePrimitive,
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{
        NativeDeclaration, TraitDef, TypeDefKind, native::NativeStorageLayout,
        verify::validate_native_declarations,
    },
    language::Protocol,
    ty::Ty,
};
use std::{
    collections::{BTreeMap, HashSet},
    slice,
    sync::Arc,
};

#[derive(Debug, Clone)]
pub(crate) struct BindingRegistration {
    // The catalog retains the shared append-only scope; each binding does not freeze a prefix.
    pub(crate) declarations: Vec<NativeDeclaration>,
    pub(crate) binding: NativeBinding,
    pub(crate) required_catalog: DeclarationCatalog,
}

impl BindingRegistration {
    pub(crate) fn checked(
        declarations: Vec<NativeDeclaration>,
        binding: NativeBinding,
        required_catalog: DeclarationCatalog,
    ) -> NativeResult<Self> {
        let declarations = DefinitionMetadata::checked(
            required_catalog.definitions(),
            declarations,
            &CancellationToken::default(),
        )
        .map_err(|cause| RuntimeError::metadata_conflict(cause.to_string()))?
        .into_records();
        Ok(Self {
            declarations,
            binding,
            required_catalog,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NativeRegistry {
    pub(crate) metadata: MetadataSnapshot,
    entries: DefinitionMap<Arc<BindingRegistration>>,
    pub(crate) storage: DefinitionMap<NativeStorage>,
    pub(crate) catalog: DeclarationCatalog,
    pub(crate) path_bindings: DefinitionMap<Vec<HostPathAdapter>>,
    pub(crate) trait_references: DefinitionMap<BTreeMap<DefinitionId, TraitDef>>,
    // Only declarations published by providers, never their foreign requirements.
    pub(crate) owned_traits: HashSet<DefinitionId>,
}

impl Default for NativeRegistry {
    fn default() -> Self {
        let catalog = DeclarationCatalog::default();
        let context = catalog.types.context().clone();
        Self {
            metadata: MetadataSnapshot::default(),
            owned_traits: HashSet::new(),
            entries: DefinitionMap::new(context.clone()),
            storage: DefinitionMap::new(context.clone()),
            trait_references: DefinitionMap::new(context.clone()),
            path_bindings: DefinitionMap::new(context),
            catalog,
        }
    }
}

impl NativeRegistry {
    /// Validate carried declarations before publication, including unused entries.
    pub(crate) fn validate_installed_contracts(&self, module: &BytecodeModule) -> NativeResult<()> {
        let definitions = self.catalog.definitions();
        let expected = definitions
            .lookup_root(&module.identity)
            .and_then(|root| self.trait_references.get_id(root));
        // Every installed module has a registry entry, including an empty set.
        // Source-owned modules instead carry requirements proved against their
        // verified executable dependency closure; they are not native contracts.
        if let Some(expected) = expected
            && (expected.len() != module.trait_references.len()
                || module
                    .trait_references
                    .iter()
                    .any(|reference| expected.get(&reference.declaration) != Some(&reference.abi)))
        {
            return Err(RuntimeError::module_validation(
                "source trait references differ from their installed module contracts",
            ));
        }
        dependencies::validate_installed_traits(&self.catalog, module)?;
        dependencies::validate_installed_values(&self.catalog, module)?;
        for declaration in &module.native_declarations {
            let CallableImplementation::Native(binding) = declaration.function.implementation
            else {
                return Err(RuntimeError::module_validation(
                    "native declaration lacks an installed binding",
                ));
            };
            if self
                .entries
                .get_id(binding)
                .is_none_or(|entry| !entry.declarations.contains(declaration))
            {
                return Err(RuntimeError::module_validation(
                    "native declaration differs from its installed contract",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn install(
        &mut self,
        registration: Arc<BindingRegistration>,
        imports: &mut CatalogImports,
    ) -> NativeResult<()> {
        let invalid = || RuntimeError::metadata_conflict("invalid or duplicate native binding");
        if registration.declarations.is_empty() || registration.declarations.len() > 4096 {
            return Err(invalid());
        }
        // Import once into the registry's shared context before comparing IDs.
        // Identity allocation is not publication; failed validation leaves the
        // catalog and entry maps unchanged.
        let context = self.entries.context();
        let registration = Arc::new(BindingRegistration {
            declarations: DefinitionMetadata::checked(
                registration.required_catalog.definitions(),
                registration.declarations.clone(),
                &CancellationToken::default(),
            )
            .and_then(|metadata| metadata.import_into(context, &CancellationToken::default()))
            .map_err(|cause| RuntimeError::metadata_conflict(cause.to_string()))?
            .into_records(),
            binding: registration.binding.clone(),
            required_catalog: imports.import(&registration.required_catalog)?,
        });
        let definitions = registration.required_catalog.definitions();
        let declarations = &registration.declarations;
        for declaration in declarations {
            let identity = definitions
                .resolve(declaration.declaration)
                .map_err(|_| invalid())?;
            validate_native_declarations(
                slice::from_ref(declaration),
                identity.module(),
                &definitions,
                &CancellationToken::default(),
            )
            .map_err(|_| invalid())?;
        }
        let declaration = declarations.first().ok_or_else(invalid)?;
        let CallableImplementation::Native(binding) = &declaration.function.implementation else {
            return Err(invalid());
        };
        let id = *binding;
        if self.entries.get_id(id).is_some() {
            return Err(invalid());
        }
        let mut seen = HashSet::new();
        for declaration in declarations {
            if declaration.function.implementation != CallableImplementation::Native(id)
                || !seen.insert(&declaration.declaration)
            {
                return Err(invalid());
            }
            registration.binding.check_policies(&declaration.function)?;
            registration.binding.check(
                &Signature {
                    params: declaration
                        .function
                        .params
                        .iter()
                        .map(|p| p.ty.clone())
                        .collect(),
                    result: declaration
                        .concrete_result
                        .clone()
                        .unwrap_or_else(|| declaration.function.return_type.clone()),
                },
                &definitions,
                &registration.required_catalog,
            )?;
        }
        let mut catalog = self.catalog.clone();
        for declaration in declarations {
            catalog.insert_declaration(declaration.clone())?;
        }
        self.entries
            .insert_id(id, registration)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        self.catalog = catalog;
        Ok(())
    }

    pub(crate) fn link(
        &self,
        carrier: ModuleRef,
        slot: NativeImportId,
        program: &VerifiedProgram,
    ) -> NativeResult<Arc<LinkedNativeFunction>> {
        let import = program
            .modules()
            .get(carrier.index())
            .and_then(|module| module.native_imports.get(slot.index()))
            .ok_or_else(|| RuntimeError::module_validation("native import carrier"))?;
        let entry = self
            .entries
            .get_id(import.binding)
            .ok_or_else(|| RuntimeError::module_validation("native binding is not installed"))?;
        dependencies::validate(&entry.required_catalog, program)?;
        let declaration = program
            .modules()
            .iter()
            .find(|module| {
                module.identity
                    == *program
                        .definitions()
                        .resolve(import.instance.declaration)
                        .expect("verified native definition")
                        .module()
            })
            .and_then(|module| {
                module
                    .native_declarations
                    .iter()
                    .find(|declaration| declaration.declaration == import.instance.declaration)
            })
            .ok_or_else(|| RuntimeError::module_validation("native declaration is absent"))?;
        if !import.structurally_valid(program.definitions())
            || !entry.declarations.contains(declaration)
            || declaration.function.implementation != CallableImplementation::Native(import.binding)
        {
            return Err(RuntimeError::module_validation(
                "native binding differs from its registered contract",
            ));
        }
        let mut signature = import.signature.clone();
        if let Some(adapter) = &import.result_adapter {
            signature.result = adapter.receiver.clone();
        }
        if let Ty::NativeObject(nominal) = &signature.result {
            let declared = self
                .catalog
                .types
                .get_id(nominal.declaration)
                .ok_or_else(|| {
                    RuntimeError::module_validation("native result type is not installed")
                })?;
            // The verified carrier/import admits the template under its exact binder.
            // Closed signature preparation and shared application retain those facts;
            // payload construction need not rediscover this witness on every call.
            if declared.kind == TypeDefKind::NativeStorage(NativeStorageLayout::Opaque)
                && !program.native_applications().iter().any(|witness| {
                    witness.carrier() == carrier
                        && witness.occurrence() == NativeApplicationOccurrence::Import(slot)
                        && witness.nominal() == nominal
                        && witness.binder() == import.generic.as_ref()
                })
            {
                return Err(RuntimeError::module_validation(
                    "native result occurrence witness",
                ));
            }
        }
        entry
            .binding
            .check(&signature, program.definitions(), &self.catalog)?;
        let selected = import
            .callables
            .iter()
            .map(|operation| {
                let OperationWitness::Selected(callable) = operation else {
                    return Ok(LinkedOperation::Forward(operation.requirement().clone()));
                };
                let (slot, owner) = program
                    .modules()
                    .iter()
                    .enumerate()
                    .find(|(_, owner)| {
                        owner.identity
                            == *program
                                .definitions()
                                .resolve(callable.instance.declaration)
                                .expect("verified callable definition")
                                .module()
                    })
                    .ok_or_else(|| RuntimeError::module_validation("native callable owner"))?;
                let target = match &callable.implementation {
                    CallableImplementation::Script => owner
                        .functions
                        .iter()
                        .find(|function| function.identity.as_ref() == Some(&callable.instance))
                        .map(|function| CallableTarget::Script(function.id)),
                    CallableImplementation::Native(binding) => owner
                        .native_imports
                        .iter()
                        .position(|import| {
                            import.instance == callable.instance
                                && import.binding == *binding
                                && import.signature == callable.signature
                        })
                        .map(|slot| CallableTarget::Native(NativeImportId::new(slot))),
                    CallableImplementation::Required | CallableImplementation::NativeDefault(_) => {
                        None
                    }
                }
                .ok_or_else(|| RuntimeError::module_validation("native callable target"))?;
                Ok(LinkedOperation::Ready(LinkedCallable {
                    environment: None,
                    scoped_signature: None,
                    owner: CallableOwner::Program(ModuleRef::new(slot)),
                    target,
                    params: callable.signature.params.clone().into_boxed_slice(),
                    result: callable.signature.result.clone(),
                    primitive: callable_primitive(callable, program.definitions()),
                }))
            })
            .collect::<NativeResult<Vec<_>>>()?;
        Ok(Arc::new(LinkedNativeFunction {
            import: slot,
            environment: None,
            declaration: import.instance.declaration,
            binding: entry.binding.clone(),
            signature,
            result_adapter: LinkedResultAdapter::link(import, program)?,
            prepared_signature: Default::default(),
            selected: selected.into_boxed_slice(),
        }))
    }
}

pub(crate) fn callable_primitive(
    callable: &NativeCallableApplication,
    table: &DefinitionTable,
) -> Option<RuntimePrimitive> {
    if callable.origin == NativeCallableOrigin::ProtocolAdapter
        && callable
            .signature
            .params
            .iter()
            .all(|ty| matches!(ty, Ty::Builtin(_)))
    {
        let member = &callable.requirement.member;
        [
            (Protocol::PartialEq, "eq", RuntimePrimitive::ValueEq),
            (Protocol::Hash, "hash", RuntimePrimitive::ValueHash),
            (Protocol::Ord, "cmp", RuntimePrimitive::ValueCmp),
        ]
        .into_iter()
        .find(|(protocol, name, _)| {
            table
                .parent(*member)
                .ok()
                .flatten()
                .is_some_and(|parent| Protocol::from_definition(&parent, table) == Some(*protocol))
                && table
                    .resolve(*member)
                    .ok()
                    .and_then(|view| view.segments().last())
                    .is_some_and(|part| {
                        part.kind == DefinitionKind::Method
                            && part.occurrence == 0
                            && part.name == *name
                    })
        })
        .map(|(_, _, primitive)| primitive)
    } else {
        None
    }
}

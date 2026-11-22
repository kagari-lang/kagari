//! Validate registrations and prepare concrete entries/callables before execution.

mod authored;
mod dependencies;
mod forwarding;
mod foundation;
mod matching;
mod scripts;
#[cfg(test)]
mod tests;
use crate::{
    error::RuntimeError,
    host::HostPathAdapter,
    metadata::native::MetadataSnapshot,
    module::{VerifiedProgram, admission::DefinitionAdmission},
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
    program::ModuleRef,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        map::DefinitionMap,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    callable::witness::OperationWitness,
    native_import::{
        NativeImport,
        callables::{NativeCallableApplication, NativeCallableOrigin},
        foundation::FoundationNative,
    },
    standard::RuntimePrimitive,
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{NativeDeclaration, TraitDef, verify::validate_native_declarations},
    language::Protocol,
    ty::Ty,
};
use std::{
    collections::{BTreeMap, HashSet},
    slice,
    sync::{Arc, OnceLock},
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
    declaration_bindings: DefinitionMap<DefinitionId>,
    pub(crate) storage: DefinitionMap<NativeStorage>,
    pub(crate) catalog: DeclarationCatalog,
    scripts: DefinitionMap<scripts::InstalledScript>,
    authored_requirements: BTreeMap<ModuleIdentity, DeclarationCatalog>,
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
            scripts: DefinitionMap::new(context.clone()),
            authored_requirements: BTreeMap::new(),
            owned_traits: HashSet::new(),
            entries: DefinitionMap::new(context.clone()),
            declaration_bindings: DefinitionMap::new(context.clone()),
            storage: DefinitionMap::new(context.clone()),
            trait_references: DefinitionMap::new(context.clone()),
            path_bindings: DefinitionMap::new(context),
            catalog,
        }
    }
}

/// One cold link/revalidation pass over a fixed program and immutable registry.
/// The registry owns every keyed Arc for the entire pass; keys cannot be recycled.
/// A new pass starts empty, so no authority crosses publication or recipients.
#[derive(Default)]
pub(crate) struct DependencyValidation {
    catalogs: HashSet<(usize, usize, usize, usize)>,
}

impl DependencyValidation {
    fn validate(
        &mut self,
        required: &DeclarationCatalog,
        program: &VerifiedProgram,
        admission: &DefinitionAdmission,
    ) -> NativeResult<()> {
        let key = (
            Arc::as_ptr(&required.types) as usize,
            Arc::as_ptr(&required.traits) as usize,
            Arc::as_ptr(&required.declarations) as usize,
            Arc::as_ptr(&required.implementations) as usize,
        );
        if !self.catalogs.contains(&key) {
            dependencies::validate(required, program, admission)?;
            self.catalogs.insert(key);
        }
        Ok(())
    }
}

impl NativeRegistry {
    /// Validate carried declarations before publication, including unused entries.
    pub(crate) fn validate_installed_contracts(
        &self,
        module: &BytecodeModule,
        program: &VerifiedProgram,
        admission: &DefinitionAdmission,
    ) -> NativeResult<()> {
        if let Some(required) = self.authored_requirements.get(&module.identity) {
            authored::validate(required, module, program, admission)?;
        }
        let definitions = self.catalog.definitions();
        if let Some(expected) = definitions
            .lookup_root(&module.identity)
            .and_then(|root| self.trait_references.get_id(root))
        {
            dependencies::validate_expected_traits(expected, module, admission)?;
        }
        dependencies::validate_installed_traits(&self.catalog, module, admission)?;
        dependencies::validate_installed_values(&self.catalog, module, program, admission)?;
        self.validate_forwarding(module, admission)?;
        self.validate_scripts(module, program, admission)?;
        for declaration in &module.native_declarations {
            let declaration = admission.runtime(declaration, &CancellationToken::default())?;
            let CallableImplementation::Native(binding) = declaration.function.implementation
            else {
                return Err(RuntimeError::module_validation(
                    "native declaration lacks an installed binding",
                ));
            };
            if let Some(entry) =
                FoundationNative::for_id(declaration.declaration, admission.target())
            {
                if !entry.declaration_matches(&declaration, admission.target())
                    || !entry.schema_matches(
                        &module.identity,
                        &module.public_items,
                        program.definitions(),
                    )
                {
                    return Err(RuntimeError::module_validation(
                        "invalid foundation native declaration",
                    ));
                }
                continue;
            }
            let entry = self
                .declaration_bindings
                .get_id(declaration.declaration)
                .or_else(|| self.declaration_bindings.get_id(binding))
                .and_then(|entry| self.entries.get_id(*entry))
                .ok_or_else(|| {
                    RuntimeError::module_validation("native declaration is not installed")
                })?;
            if !entry
                .declarations
                .iter()
                .any(|expected| matching::installed(expected, &declaration, program, admission))
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
        if self.entries.get_id(id).is_some()
            || FoundationNative::for_id(id, &definitions).is_some()
            || declarations.iter().any(|declaration| {
                FoundationNative::for_id(declaration.declaration, &definitions).is_some()
            })
        {
            return Err(invalid());
        }
        let mut seen = HashSet::new();
        for declaration in declarations {
            if declaration.function.implementation != CallableImplementation::Native(id)
                || !seen.insert(&declaration.declaration)
                || self
                    .declaration_bindings
                    .get_id(declaration.declaration)
                    .is_some()
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
            )?;
        }
        let mut catalog = self.catalog.clone();
        for declaration in declarations {
            catalog.insert_declaration(declaration.clone())?;
        }
        for declaration in declarations {
            self.declaration_bindings
                .insert_id(declaration.declaration, id)
                .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        }
        self.entries
            .insert_id(id, registration)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        self.catalog = catalog;
        Ok(())
    }

    pub(crate) fn link(
        &self,
        import: &NativeImport,
        importer: &BytecodeModule,
        program: &VerifiedProgram,
        admission: &DefinitionAdmission,
        validated: &mut DependencyValidation,
    ) -> NativeResult<Arc<LinkedNativeFunction>> {
        let definitions = program.definitions();
        if !FoundationNative::import_allowed(import, &importer.identity, definitions) {
            return Err(RuntimeError::module_validation(
                "private foundation native import",
            ));
        }
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
        let mismatch = || {
            RuntimeError::module_validation("native binding differs from its registered contract")
        };
        if !import.structurally_valid(definitions)
            || declaration.function.implementation != CallableImplementation::Native(import.binding)
        {
            return Err(mismatch());
        }
        let binding =
            if let Some(entry) = FoundationNative::for_id(declaration.declaration, definitions) {
                if !entry.declaration_matches(declaration, definitions) {
                    return Err(mismatch());
                }
                foundation::binding(entry)
            } else {
                let declaration_id = admission.runtime_id(import.instance.declaration)?;
                let binding_id = admission.runtime_id(import.binding)?;
                let entry = self
                    .declaration_bindings
                    .get_id(declaration_id)
                    .or_else(|| self.declaration_bindings.get_id(binding_id))
                    .and_then(|binding| self.entries.get_id(*binding))
                    .ok_or_else(|| {
                        RuntimeError::module_validation("native binding is not installed")
                    })?;
                validated.validate(&entry.required_catalog, program, admission)?;
                let registered = admission.runtime(declaration, &CancellationToken::default())?;
                if !entry
                    .declarations
                    .iter()
                    .any(|expected| matching::installed(expected, &registered, program, admission))
                {
                    return Err(mismatch());
                }
                entry.binding.clone()
            };
        let mut signature = import.signature.clone();
        if let Some(adapter) = &import.result_adapter {
            signature.result = adapter.receiver.clone();
        }
        binding.check(
            &admission.runtime(&signature, &CancellationToken::default())?,
            admission.target(),
        )?;
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
                    CallableImplementation::Required => None,
                }
                .ok_or_else(|| RuntimeError::module_validation("native callable target"))?;
                Ok(LinkedOperation::Ready(LinkedCallable {
                    environment: None,
                    scoped_signature: OnceLock::new(),
                    owner: CallableOwner::Program(ModuleRef::new(slot)),
                    target,
                    params: callable.signature.params.clone().into_boxed_slice(),
                    result: callable.signature.result.clone(),
                    primitive: callable_primitive(callable, program.definitions()),
                }))
            })
            .collect::<NativeResult<Vec<_>>>()?;
        Ok(Arc::new(LinkedNativeFunction {
            declaration: import.instance.declaration,
            binding,
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

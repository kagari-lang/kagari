//! Validate registrations and prepare concrete entries/callables before execution.

mod dependencies;
use crate::{
    error::RuntimeError,
    host::HostFunctionId,
    module::VerifiedProgram,
    native::{
        binding::{Codec, LinkedNativeFunction, NativeBinding, NativeResult},
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
        DefinitionKind, map::DefinitionMap, metadata::DefinitionMetadata, table::DefinitionTable,
    },
};
use kagari_contract::{
    callable::witness::OperationWitness,
    native_import::{
        NativeImport,
        callables::{NativeCallableApplication, NativeCallableOrigin},
    },
    standard::RuntimePrimitive,
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{NativeDeclaration, verify::validate_native_declarations},
    language::Protocol,
    ty::Ty,
};
use std::{collections::HashSet, slice, sync::Arc};

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

pub(crate) fn link_host(
    import: &NativeImport,
    binding: HostFunctionId,
) -> Arc<LinkedNativeFunction> {
    let signature = import.signature.clone();
    let entry = NativeBinding::new(
        vec![Codec::Value; signature.params.len()],
        Codec::Value,
        move |context| {
            let arguments = (0..context.arguments.len())
                .map(|slot| context.argument(slot))
                .collect::<NativeResult<Vec<_>>>()?;
            context.runtime.invoke_bound_host(binding, &arguments)
        },
    );
    Arc::new(LinkedNativeFunction {
        declaration: import.instance.declaration,
        binding: entry,
        signature,
        prepared_signature: Default::default(),
        selected: Box::new([]),
        result_adapter: None,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct NativeRegistry {
    entries: DefinitionMap<Arc<BindingRegistration>>,
    pub(crate) storage: DefinitionMap<NativeStorage>,
    pub(crate) catalog: DeclarationCatalog,
}

impl Default for NativeRegistry {
    fn default() -> Self {
        let catalog = DeclarationCatalog::default();
        let context = catalog.types.context().clone();
        Self {
            entries: DefinitionMap::new(context.clone()),
            storage: DefinitionMap::new(context),
            catalog,
        }
    }
}

impl NativeRegistry {
    pub(crate) fn validate_installed_traits(&self, module: &BytecodeModule) -> NativeResult<()> {
        dependencies::validate_installed_traits(&self.catalog, module)
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
        import: &NativeImport,
        program: &VerifiedProgram,
    ) -> NativeResult<Arc<LinkedNativeFunction>> {
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
        entry
            .binding
            .check(&signature, program.definitions(), &entry.required_catalog)?;
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

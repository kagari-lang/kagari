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
        DefinitionKind,
        map::DefinitionMap,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    callable::{CallableImplementation, witness::OperationWitness},
    language::Protocol,
    native_import::{
        NativeImport, NativeSignature,
        callables::{NativeCallableApplication, NativeCallableOrigin},
    },
    standard::RuntimePrimitive,
    types::{NativeDeclaration, Ty, verify::validate_native_declarations},
};
use std::{collections::HashSet, rc::Rc, slice};

#[derive(Debug, Clone)]
pub(crate) struct BindingRegistration {
    // The catalog retains the shared append-only scope; each binding does not freeze a prefix.
    pub(crate) declarations: Vec<NativeDeclaration<DefinitionId>>,
    pub(crate) binding: NativeBinding,
    pub(crate) required_catalog: DeclarationCatalog,
}

impl BindingRegistration {
    pub(crate) fn checked(
        declarations: Vec<NativeDeclaration>,
        binding: NativeBinding,
        required_catalog: DeclarationCatalog,
    ) -> NativeResult<Self> {
        let records = required_catalog.scope(&declarations)?;
        let declarations = DefinitionMetadata::checked(
            required_catalog.definitions(),
            records,
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
    import: &NativeImport<DefinitionId>,
    binding: HostFunctionId,
) -> Rc<LinkedNativeFunction> {
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
    Rc::new(LinkedNativeFunction {
        binding: entry,
        signature,
        scoped_signature: None,
        selected: Box::new([]),
        result_adapter: None,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct NativeRegistry {
    entries: DefinitionMap<Rc<BindingRegistration>>,
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
    pub(crate) fn validate_installed_traits(
        &self,
        module: &BytecodeModule<DefinitionId>,
    ) -> NativeResult<()> {
        dependencies::validate_installed_traits(&self.catalog, module)
    }

    pub(crate) fn install(
        &mut self,
        registration: Rc<BindingRegistration>,
        imports: &mut CatalogImports,
    ) -> NativeResult<()> {
        let invalid = || RuntimeError::metadata_conflict("invalid or duplicate native binding");
        let declarations = registration
            .required_catalog
            .paths(&registration.declarations)?;
        let declaration = declarations.first().ok_or_else(invalid)?;
        let CallableImplementation::Native(binding) = &declaration.function.implementation else {
            return Err(invalid());
        };
        let id = binding.clone();
        if self.entries.contains_key(&id) || declarations.len() > 4096 {
            return Err(invalid());
        }
        let mut seen = HashSet::new();
        for declaration in &declarations {
            validate_native_declarations(
                slice::from_ref(declaration),
                &declaration.declaration.module,
                &CancellationToken::default(),
            )
            .map_err(|_| invalid())?;
            if declaration.function.implementation != CallableImplementation::Native(id.clone())
                || !seen.insert(&declaration.declaration)
            {
                return Err(invalid());
            }
            registration.binding.check(
                &NativeSignature {
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
                &registration.required_catalog,
            )?;
        }
        let mut catalog = self.catalog.clone();
        for declaration in &declarations {
            catalog.insert_declaration(declaration.clone())?;
        }
        let context = self.entries.context();
        let registration = Rc::new(BindingRegistration {
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
        self.catalog = catalog;
        self.entries
            .insert(id, registration)
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        Ok(())
    }

    pub(crate) fn link(
        &self,
        import: &NativeImport<DefinitionId>,
        program: &VerifiedProgram,
    ) -> NativeResult<Rc<LinkedNativeFunction>> {
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
        let authored = program.paths(import)?;
        if !authored.structurally_valid()
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
            .check(&program.paths(&signature)?, &entry.required_catalog)?;
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
        Ok(Rc::new(LinkedNativeFunction {
            binding: entry.binding.clone(),
            signature,
            result_adapter: LinkedResultAdapter::link(import, program)?,
            scoped_signature: None,
            selected: selected.into_boxed_slice(),
        }))
    }
}

pub(crate) fn callable_primitive(
    callable: &NativeCallableApplication<DefinitionId>,
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
            table.parent(*member).ok().flatten().is_some_and(|parent| {
                Protocol::from_reference(&parent, Some(table)) == Some(*protocol)
            }) && table
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

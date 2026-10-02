//! Validate registrations and prepare concrete entries/callables before execution.
mod dependencies;
use crate::{
    error::RuntimeError,
    host::HostFunctionId,
    module::VerifiedProgram,
    native::{
        binding::{Codec, LinkedNativeFunction, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        context::{CallableOwner, LinkedCallable, LinkedOperation},
        result::LinkedResultAdapter,
        storage::NativeStorage,
    },
};
use kagari_abi::{
    callable::{CallableImplementation, witness::OperationWitness},
    declaration::ModuleDecl,
    language::{self, Protocol},
    native_import::callables::{NativeCallableApplication, NativeCallableOrigin},
    native_import::{NativeImport, NativeSignature},
    standard::RuntimePrimitive,
    types::{AbiType, NativeDeclaration, verify::validate_native_declarations},
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget, program::ModuleRef};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionId};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    slice,
};

#[derive(Debug, Clone)]
pub(crate) struct BindingRegistration {
    pub(crate) declarations: Vec<NativeDeclaration>,
    pub(crate) binding: NativeBinding,
    pub(crate) required_catalog: DeclarationCatalog,
}

pub(crate) fn link_host(
    import: &NativeImport,
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

#[derive(Debug, Clone, Default)]
pub(crate) struct NativeRegistry {
    entries: HashMap<DefinitionId, Rc<BindingRegistration>>,
    pub(crate) storage: HashMap<DefinitionId, NativeStorage>,
    pub(crate) catalog: DeclarationCatalog,
}
impl NativeRegistry {
    pub(crate) fn install(&mut self, registration: BindingRegistration) -> NativeResult<()> {
        let invalid = || RuntimeError::metadata_conflict("invalid or duplicate native binding");
        let declaration = registration.declarations.first().ok_or_else(invalid)?;
        let CallableImplementation::Native(binding) = &declaration.function.implementation else {
            return Err(invalid());
        };
        let id = binding.clone();
        if self.entries.contains_key(&id) || registration.declarations.len() > 4096 {
            return Err(invalid());
        }
        let mut seen = HashSet::new();
        for declaration in &registration.declarations {
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
        for declaration in &registration.declarations {
            catalog.insert_declaration(declaration.clone())?;
        }
        self.catalog = catalog;
        self.entries.insert(id, Rc::new(registration));
        Ok(())
    }
    pub(crate) fn link(
        &self,
        import: &NativeImport,
        program: &VerifiedProgram,
    ) -> NativeResult<Rc<LinkedNativeFunction>> {
        let entry = self
            .entries
            .get(&import.binding)
            .ok_or_else(|| RuntimeError::module_validation("native binding is not installed"))?;
        dependencies::validate(&entry.required_catalog, program)?;
        let declaration = program
            .modules()
            .iter()
            .find(|module| module.identity == import.instance.declaration.module)
            .and_then(|module| {
                module
                    .native_declarations
                    .iter()
                    .find(|declaration| declaration.declaration == import.instance.declaration)
            })
            .ok_or_else(|| RuntimeError::module_validation("native declaration is absent"))?;
        if !import.structurally_valid()
            || !entry.declarations.contains(declaration)
            || declaration.function.implementation
                != CallableImplementation::Native(import.binding.clone())
        {
            return Err(RuntimeError::module_validation(
                "native binding differs from its registered contract",
            ));
        }
        let mut signature = import.signature.clone();
        if let Some(adapter) = &import.result_adapter {
            signature.result = adapter.receiver.clone();
        }
        entry.binding.check(&signature, &entry.required_catalog)?;
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
                    .find(|(_, owner)| owner.identity == callable.instance.declaration.module)
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
                    primitive: callable_primitive(callable),
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

pub(crate) fn callable_primitive(callable: &NativeCallableApplication) -> Option<RuntimePrimitive> {
    if callable.origin == NativeCallableOrigin::ProtocolAdapter
        && callable
            .signature
            .params
            .iter()
            .all(|ty| matches!(ty, AbiType::Builtin(_)))
    {
        let member = &callable.requirement.member;
        [
            (Protocol::PartialEq, "eq", RuntimePrimitive::ValueEq),
            (Protocol::Hash, "hash", RuntimePrimitive::ValueHash),
            (Protocol::Ord, "cmp", RuntimePrimitive::ValueCmp),
        ]
        .into_iter()
        .find(|(protocol, name, _)| {
            ModuleDecl::method_id(&language::identity(*protocol), name) == *member
        })
        .map(|(_, _, primitive)| primitive)
    } else {
        None
    }
}

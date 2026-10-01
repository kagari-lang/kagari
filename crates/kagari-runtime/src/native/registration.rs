//! IDs resolve to trusted registrations; signatures come from checked declarations.
use crate::{
    error::RuntimeError,
    host::HostFunctionId,
    module::VerifiedProgram,
    native::{NativeAction, NativeContext, NativeInvocationState, catalog::NativeCatalog},
};
use kagari_abi::{
    callable::CallableImplementation,
    native_api::NativeModule,
    native_import::NativeImport,
    types::{
        NativeDeclaration, PublicAbiItem,
        verify::{concrete_type_valid, validate_native_declarations},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionId};
use std::{
    collections::{HashMap, HashSet},
    fmt, iter,
    rc::Rc,
    slice,
};

pub type NativeEntry =
    dyn Fn(&mut NativeContext<'_>) -> Result<Box<dyn NativeInvocationState>, RuntimeError>;
#[derive(Clone)]
pub struct NativeRegistration {
    /// Ordinary declaration metadata exported by the source compiler or host adapter.
    /// Multiple declarations may name one entry, without another signature template.
    pub declarations: Vec<NativeDeclaration>,
    pub(crate) scratch_slots: usize,
    pub(crate) entry: Rc<NativeEntry>,
    pub(crate) required_catalog: NativeCatalog,
}
impl NativeRegistration {
    pub fn new(
        declarations: Vec<NativeDeclaration>,
        scratch_slots: usize,
        entry: impl Fn(&mut NativeContext<'_>) -> Result<Box<dyn NativeInvocationState>, RuntimeError>
        + 'static,
    ) -> Self {
        Self {
            declarations,
            scratch_slots,
            entry: Rc::new(entry),
            required_catalog: NativeCatalog::default(),
        }
    }
}
impl fmt::Debug for NativeRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeRegistration")
            .field("declarations", &self.declarations)
            .finish_non_exhaustive()
    }
}
fn entry_id(declaration: &NativeDeclaration) -> Option<&DefinitionId> {
    match &declaration.function.implementation {
        CallableImplementation::Native(id) => Some(id),
        _ => None,
    }
}
#[derive(Debug, Default, Clone)]
pub struct NativeRegistry {
    entries: HashMap<DefinitionId, Rc<NativeRegistration>>,
    catalog: NativeCatalog,
}
impl NativeRegistry {
    pub(crate) fn validate_defaults(&self) -> Result<(), RuntimeError> {
        self.catalog.validate_defaults()
    }
    pub(crate) fn check_implementations<'a>(
        &self,
        modules: impl IntoIterator<Item = &'a NativeModule>,
    ) -> Result<(), RuntimeError> {
        self.catalog.check_implementations(modules)
    }
    pub(crate) fn install_catalog(&mut self, catalog: NativeCatalog) -> Result<(), RuntimeError> {
        if catalog
            .traits
            .keys()
            .any(|id| self.catalog.get(id).is_some())
        {
            return Err(RuntimeError::metadata_conflict(
                "duplicate native declaration",
            ));
        }
        self.catalog.merge(&catalog)
    }
    pub(crate) fn require_catalog(&self, required: &NativeCatalog) -> Result<(), RuntimeError> {
        if required.satisfied_by(&self.catalog) {
            Ok(())
        } else {
            Err(RuntimeError::metadata_conflict(
                "missing or changed native contract dependency",
            ))
        }
    }
    pub fn install(&mut self, registration: NativeRegistration) -> Result<(), RuntimeError> {
        let invalid =
            || RuntimeError::metadata_conflict("duplicate or invalid native registration");
        let id = registration
            .declarations
            .first()
            .and_then(entry_id)
            .ok_or_else(invalid)?
            .clone();
        if registration.scratch_slots > 4096
            || registration.declarations.len() > 4096
            || !id.within_path_limit()
            || self.entries.contains_key(&id)
        {
            return Err(invalid());
        }
        let cancel = CancellationToken::default();
        let mut seen = HashSet::new();
        for declaration in &registration.declarations {
            validate_native_declarations(
                slice::from_ref(declaration),
                &declaration.declaration.module,
                &cancel,
            )
            .map_err(|_| invalid())?;
            let function = &declaration.function;
            if entry_id(declaration) != Some(&id)
                || !declaration.declaration.within_path_limit()
                || !seen.insert(&declaration.declaration)
                || function.generic_params.len() > 4096
                || function.params.len() > 4096
                || function.bounds.len() > 4096
                || function
                    .bounds
                    .iter()
                    .any(|bound| bound.constraints.len() > 4096)
            {
                return Err(invalid());
            }
            for ty in function
                .params
                .iter()
                .map(|p| &p.ty)
                .chain(iter::once(&function.return_type))
            {
                // Templates have validated binders and may retain projections.
                // Their concrete layouts are checked by verified program linking.
                if !ty.within_wire_limits()
                    || (ty.is_concrete() && !concrete_type_valid(ty, &cancel))
                {
                    return Err(invalid());
                }
            }
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
    ) -> Result<Rc<NativeRegistration>, RuntimeError> {
        let entry = self
            .entries
            .get(&import.binding)
            .ok_or_else(|| RuntimeError::module_validation("native entry is not installed"))?;
        if entry.required_catalog.traits.iter().any(|(id, expected)| {
            !program.modules().iter().any(|module| {
                module.identity == id.module && module.public_items.iter().any(|item| {
                    matches!(item, PublicAbiItem::Trait(contract) if contract == expected)
                })
            })
        }) {
            return Err(RuntimeError::module_validation(
                "native dependency differs from its registered trait contract",
            ));
        }
        if entry
            .required_catalog
            .declarations
            .iter()
            .any(|(id, expected)| {
                !program.modules().iter().any(|module| {
                    module.identity == id.module
                        && module
                            .native_declarations
                            .iter()
                            .any(|declaration| declaration == expected)
                })
            })
        {
            return Err(RuntimeError::module_validation(
                "native dependency differs from its registered template contract",
            ));
        }
        let declaration = program
            .modules()
            .iter()
            .find(|owner| owner.identity == import.instance.declaration.module)
            .and_then(|owner| {
                owner
                    .native_declarations
                    .iter()
                    .find(|declaration| declaration.declaration == import.instance.declaration)
            })
            .ok_or_else(|| {
                RuntimeError::module_validation("missing verified native declaration")
            })?;
        // The sealed VerifiedProgram already checks applied signatures and bounds
        // against its dependency closure. Installation must match the complete
        // template, including bounds; an empty catalog cannot prove those facts.
        if !import.structurally_valid()
            || import.instance.declaration != declaration.declaration
            || entry_id(declaration) != Some(&import.binding)
            || !entry.declarations.contains(declaration)
        {
            return Err(RuntimeError::module_validation(
                "native import differs from its installed declaration",
            ));
        }
        Ok(entry.clone())
    }
}

struct HostEntry;
impl NativeInvocationState for HostEntry {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        context
            .retained(0)
            .map(NativeAction::Complete)
            .ok_or_else(|| RuntimeError::module_validation("host native result root"))
    }
}
pub(crate) fn host_registration(
    import: &NativeImport,
    binding: HostFunctionId,
) -> Rc<NativeRegistration> {
    debug_assert!(import.host.is_some());
    // The host registry has already matched the installed declaration and its
    // authority/borrow rules. The handler uses the common rooted invocation driver.
    Rc::new(NativeRegistration::new(vec![], 1, move |context| {
        let arguments = (0..context.signature().params.len())
            .map(|slot| context.argument(slot))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| RuntimeError::module_validation("host native arguments"))?;
        let result = context.runtime.invoke_bound_host(binding, &arguments)?;
        context.retain(0, result)?;
        Ok(Box::new(HostEntry))
    }))
}

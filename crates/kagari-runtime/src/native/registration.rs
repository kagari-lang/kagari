//! IDs resolve to trusted registrations; signatures come from checked declarations.
use crate::{
    error::RuntimeError,
    host::HostFunctionId,
    native::{NativeAction, NativeContext, NativeInvocationState},
};
use kagari_abi::{
    callable::CallableImplementation,
    native_import::NativeImport,
    scalar::BuiltinType,
    types::{
        AbiType, NativeDeclaration,
        proofs::ProofCatalog,
        substitution::TypeSubstitution,
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
}
impl NativeRegistry {
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
                || function
                    .bounds
                    .iter()
                    .any(|bound| !bound.constraints.is_empty())
            {
                return Err(invalid());
            }
            let mut substitution = TypeSubstitution::default();
            for parameter in &function.generic_params {
                substitution.bind(
                    &parameter.owner,
                    parameter.position,
                    &AbiType::Builtin(BuiltinType::Unit),
                );
            }
            for ty in function
                .params
                .iter()
                .map(|p| &p.ty)
                .chain(iter::once(&function.return_type))
            {
                let concrete = substitution.apply(ty, &cancel).map_err(|_| invalid())?;
                if !concrete.within_wire_limits() || !concrete_type_valid(&concrete, &cancel) {
                    return Err(invalid());
                }
            }
        }
        self.entries.insert(id, Rc::new(registration));
        Ok(())
    }
    pub(crate) fn link(
        &self,
        import: &NativeImport,
    ) -> Result<Rc<NativeRegistration>, RuntimeError> {
        let entry = self
            .entries
            .get(&import.binding)
            .ok_or_else(|| RuntimeError::module_validation("native entry is not installed"))?;
        let cancel = CancellationToken::default();
        let catalog = ProofCatalog::new(vec![], vec![], [], [], &cancel)
            .map_err(|_| RuntimeError::module_validation("native declaration catalog"))?;
        if !import.structurally_valid()
            || !entry.declarations.iter().any(|declaration| {
                import
                    .matches_declaration(declaration, &catalog, &cancel)
                    .unwrap_or(false)
            })
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

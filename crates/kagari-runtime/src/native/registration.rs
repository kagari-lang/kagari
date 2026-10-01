use crate::{
    RuntimeError,
    host::HostFunctionId,
    native::{NativeAction, NativeContext, NativeInvocationState},
};
use kagari_abi::{
    native_import::NativeImport,
    provider::{NativeBindingKey, NativeContract},
    scalar::BuiltinType,
    types::{AbiType, verify::concrete_type_valid},
};
use kagari_common::cancellation::CancellationToken;
use std::{collections::HashMap, fmt, iter, rc::Rc};

pub type NativeEntry =
    dyn Fn(&mut NativeContext<'_>) -> Result<Box<dyn NativeInvocationState>, RuntimeError>;
pub struct NativeRegistration {
    pub contract: NativeContract,
    pub(crate) scratch_slots: usize,
    pub(crate) entry: Rc<NativeEntry>,
}
impl NativeRegistration {
    pub fn new(
        contract: NativeContract,
        scratch_slots: usize,
        entry: impl Fn(&mut NativeContext<'_>) -> Result<Box<dyn NativeInvocationState>, RuntimeError>
        + 'static,
    ) -> Self {
        Self {
            contract,
            scratch_slots,
            entry: Rc::new(entry),
        }
    }
}
impl fmt::Debug for NativeRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeRegistration")
            .field("contract", &self.contract)
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Default)]
pub struct NativeRegistry {
    entries: HashMap<NativeBindingKey, Rc<NativeRegistration>>,
}
impl NativeRegistry {
    pub fn install(&mut self, registration: NativeRegistration) -> Result<(), RuntimeError> {
        if registration.scratch_slots > 4096
            || registration.contract.generic_count > 4096
            || registration.contract.host.is_some()
            || self.entries.contains_key(&registration.contract.key)
        {
            return Err(RuntimeError::metadata_conflict(
                "duplicate or invalid native registration",
            ));
        }
        let arguments =
            vec![AbiType::Builtin(BuiltinType::Unit); registration.contract.generic_count];
        let signature = registration
            .contract
            .apply(&arguments)
            .ok_or_else(|| RuntimeError::metadata_conflict("invalid native contract template"))?;
        let cancel = CancellationToken::default();
        if !signature
            .params
            .iter()
            .chain(iter::once(&signature.result))
            .all(|ty| concrete_type_valid(ty, &cancel))
        {
            return Err(RuntimeError::metadata_conflict(
                "unbound native contract template",
            ));
        }
        self.entries
            .insert(registration.contract.key, Rc::new(registration));
        Ok(())
    }
    pub(crate) fn link(
        &self,
        import: &NativeImport,
    ) -> Result<Rc<NativeRegistration>, RuntimeError> {
        let entry = self.entries.get(&import.contract.key).ok_or_else(|| {
            RuntimeError::module_validation("native provider or entry is not installed")
        })?;
        if entry.contract != import.contract || !import.structurally_valid() {
            return Err(RuntimeError::module_validation(
                "native import differs from the installed provider contract",
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
    Rc::new(NativeRegistration::new(
        import.contract.clone(),
        1,
        move |context| {
            let arguments = (0..context.signature().params.len())
                .map(|slot| context.argument(slot))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| RuntimeError::module_validation("host native arguments"))?;
            let result = context.runtime.invoke_bound_host(binding, &arguments)?;
            context.retain(0, result)?;
            Ok(Box::new(HostEntry))
        },
    ))
}

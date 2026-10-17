//! The source print helper and explicit host binding share one cold declaration.
use crate::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        FnDecl, Param,
        host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
        module::ModuleDecl,
    },
    scalar::BuiltinType,
    ty::Ty,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
    mapping::DefinitionMappingError, metadata::DefinitionMetadata,
};
use std::sync::OnceLock;

/// Borrow the immutable `host::log(message: String)` module contract.
///
/// This declares the print service without installing a callback or starting a
/// runtime. The host explicitly binds its implementation through ModuleBuilder.
/// Consumers import these scoped records before combining them with other facts.
/// Identity construction errors are reported before the declaration is published.
pub fn declaration() -> Result<DefinitionMetadata<ModuleDecl>, DefinitionMappingError> {
    static LOG: OnceLock<Result<DefinitionMetadata<ModuleDecl>, DefinitionMappingError>> =
        OnceLock::new();
    LOG.get_or_init(|| {
        let context = DefinitionContext::new()?;
        let identity = ModuleIdentity {
            package: PackageId("host".into()),
            path: Vec::new(),
        };
        let root = context.intern_root(&identity)?;
        let binding = context.intern_child(root, DefinitionKind::Function, "log", 0)?;
        let mut declaration = ModuleDecl::new(identity);
        declaration.functions.push(FnDecl {
            name: "log".into(),
            host_policy: Some(HostCallPolicy {
                passing: vec![HostPassingStyle::SharedBorrow],
                effects: HostFunctionEffects {
                    may_trap: true,
                    may_call_host_services: true,
                    may_mutate_host_state: true,
                    ..Default::default()
                },
            }),
            method_policy: MethodPolicy::default(),
            implementation: CallableImplementation::Native(binding),
            generic_params: Vec::new(),
            bounds: Vec::new(),
            params: vec![Param {
                name: "message".into(),
                ty: Ty::Builtin(BuiltinType::String),
                mutable: false,
            }],
            return_type: Ty::Builtin(BuiltinType::Unit),
        });
        declaration
            .documentation
            .insert(binding, "Write a message to the host log.".into());
        let definitions = declaration
            .collect_definitions(&context)
            .map_err(|_| DefinitionMappingError::InvalidContract)?;
        declaration
            .validate(&|_| None, &definitions)
            .map_err(|_| DefinitionMappingError::InvalidContract)?;
        DefinitionMetadata::checked(definitions, declaration, &Default::default())
    })
    .clone()
}

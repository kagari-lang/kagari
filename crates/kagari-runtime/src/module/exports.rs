//! Live public namespace navigation and exact executable export evidence.

use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    module::{ExecutableModule, Module, Program, VerifiedProgram},
};
use kagari_bytecode::program::exports::ExportTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{
    exports::{
        ProgramExports,
        path::{PublicPathError, PublicPathKind, PublicPathTarget},
    },
    ids::FunctionRef,
};
use kagari_types::callable::Signature;

impl VerifiedProgram {
    pub(crate) fn export_graph(&self) -> &ProgramExports {
        &self.exports
    }

    pub(crate) fn export_signature(&self, id: DefinitionId) -> Option<Signature> {
        let target = self.export_targets.get(&id)?;
        let function = &self.modules[target.module.index()].functions[target.function.index()];
        Some(Signature {
            params: function
                .metadata
                .semantic
                .params
                .values()
                .cloned()
                .collect(),
            result: function.metadata.semantic.result.clone()?,
        })
    }

    /// Resolve a public path relative to the Program root into its verified slots.
    /// Private, missing and overlong paths return `None`; aliases may target dependencies.
    pub fn exported_function(&self, path: &str) -> Option<ExportTarget> {
        let root = &self.modules[self.root.index()].identity;
        let id = self.exports.function(root, path)?;
        self.export_targets.get(&id).copied()
    }

    /// Return the separate zero-argument root `main` capability, if present.
    /// A private entry is never added to public-path lookup.
    pub fn entry_function(&self) -> Option<ExportTarget> {
        self.export_targets.get(&self.exports.entry?).copied()
    }
}

impl Program {
    /// Navigate a public module path from this installation's current root.
    /// The returned handle retains canonical namespace identity, not an executable
    /// generation or the alias spelling. Closed installations reject navigation.
    pub fn module(&self, path: &str) -> Result<Module, RuntimeError> {
        self.root().module(path)
    }
}

impl Module {
    /// Navigate relative to this canonical namespace in the current publication.
    /// Public aliases and prefix namespaces need no physical executable slot.
    /// Paths contain 1..64 ASCII identifiers separated by `::`, without
    /// `crate`/`self`/`super`. Invalid, absent and wrong-kind paths report
    /// `TypedPathValidation`; closed handles report `StaleHandle`.
    pub fn module(&self, path: &str) -> Result<Self, RuntimeError> {
        let snapshot = self.capture_current()?;
        let target = snapshot
            .root
            .program
            .code
            .export_graph()
            .resolve_public_path(&snapshot.namespace, path, PublicPathKind::Module)
            .map_err(|error| module_path_error(path, error))?;
        let PublicPathTarget::Module(namespace) = target else {
            return Err(module_path_error(path, PublicPathError::InvalidGraph));
        };
        Ok(self.namespace_handle(namespace.clone()))
    }
}

fn module_path_error(path: &str, error: PublicPathError) -> RuntimeError {
    let detail = match error {
        PublicPathError::InvalidPath => "invalid public path",
        PublicPathError::NotFound => "public path not found",
        PublicPathError::WrongTargetKind => "public path has wrong target kind",
        PublicPathError::InvalidGraph => {
            return RuntimeError::new(
                RuntimeErrorKind::EngineFault,
                format!("invalid verified namespace graph for module path {path:?}"),
            );
        }
    };
    RuntimeError::typed_path_validation(format!("{detail}: {path:?} (expected module)"))
}

impl ExecutableModule {
    /// Resolve a public authored function declaration, including unspecialized generics.
    /// The ID belongs to `definitions()`; it grants neither an executable entry nor
    /// private access. Traversal uses the same checked Program-root export namespaces
    /// as ordinary public calls, without requiring an emitted specialization slot.
    pub fn exported_function_declaration(
        &self,
        path: &str,
    ) -> Result<DefinitionId, PublicPathError> {
        let code = &self.program.code;
        let root = &code.modules[code.root.index()].identity;
        match code
            .exports
            .resolve_public_path(root, path, PublicPathKind::Function)?
        {
            PublicPathTarget::Function(declaration) => Ok(declaration),
            _ => Err(PublicPathError::InvalidGraph),
        }
    }

    /// Resolve a public Program-root path into a handle for the exact target generation.
    /// This remains rooted at the Program even when called on a dependency member.
    /// Handles kept across collection use the ordinary runtime retention APIs.
    /// Execution subsequently applies the ordinary runtime ownership and call checks.
    pub fn exported_function(&self, path: &str) -> Option<(Self, FunctionRef)> {
        let target = self.program.code.exported_function(path)?;
        Some((self.member(target.module)?, target.function))
    }

    /// Select the root's separately verified script entry for an explicit CLI-style call.
    /// Handles kept across collection use the ordinary runtime retention APIs.
    /// Execution subsequently applies the ordinary runtime ownership and call checks.
    pub fn entry_function(&self) -> Option<(Self, FunctionRef)> {
        let target = self.program.code.entry_function()?;
        Some((self.member(target.module)?, target.function))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Runtime, error::RuntimeErrorKind, frame::types::arguments::TypeArgument, module::Module,
        test_support::native::program::ProgramFixture,
    };
    use kagari_bytecode::{
        module::BytecodeModule,
        program::{BytecodeProgram, ModuleRef},
    };
    use kagari_common::identity::{
        DefinitionKind, DefinitionPath, ModuleIdentity, map::DefinitionContext,
        metadata::DefinitionMetadata,
    };
    use kagari_contract::exports::{ExportNamespace, ProgramExports};
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    use std::sync::Arc;

    fn navigation_program(updated: bool) -> DefinitionMetadata<BytecodeProgram> {
        let root = ModuleIdentity::single_file("navigation");
        let prefix = ModuleIdentity {
            package: root.package.clone(),
            path: vec!["internal".into()],
        };
        let users = ModuleIdentity {
            package: root.package.clone(),
            path: vec!["internal".into(), "users".into()],
        };
        let mut exports = ProgramExports {
            namespaces: [
                (
                    root.clone(),
                    ExportNamespace {
                        modules: [
                            ("api".into(), prefix.clone()),
                            ("alias".into(), prefix.clone()),
                        ]
                        .into(),
                        ..Default::default()
                    },
                ),
                (
                    prefix.clone(),
                    ExportNamespace {
                        modules: [("users".into(), users.clone())].into(),
                        ..Default::default()
                    },
                ),
                (
                    users.clone(),
                    ExportNamespace {
                        modules: [("parent".into(), prefix)].into(),
                        ..Default::default()
                    },
                ),
            ]
            .into(),
            entry: None,
        };
        if updated {
            exports
                .namespaces
                .get_mut(&users)
                .unwrap()
                .modules
                .insert("added".into(), root.clone());
        }
        // Both navigable member namespaces deliberately have no executable slot.
        ProgramFixture {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                identity: root,
                ..Default::default()
            }],
            exports,
        }
        .metadata(
            DefinitionContext::new().unwrap().snapshot(),
            &Default::default(),
        )
        .unwrap()
    }

    fn assert_live_binding_authority(runtime: &Runtime, owner: &Module, argument: &TypeArgument) {
        let invalid = DefinitionPath {
            module: owner.namespace.clone(),
            path: vec![],
        };
        let definitions = DefinitionContext::new().unwrap();
        let native = definitions
            .intern_child(
                definitions
                    .intern_root(&ModuleIdentity::single_file("unadmitted-binding"))
                    .unwrap(),
                DefinitionKind::Struct,
                "Uninstalled",
                0,
            )
            .unwrap();
        let definitions = definitions.snapshot();
        let native_path = definitions.resolve(native).unwrap().to_path();
        assert!(
            runtime
                .definition_context()
                .snapshot()
                .lookup(&native_path)
                .is_none()
        );
        // Malformed paths/declarations and unavailable constructor applications
        // must not hide failed installation admission or import foreign metadata.
        let results = [
            (
                "object",
                runtime.bind_type(owner, "missing::", &[]).map(|_| ()),
            ),
            (
                "object declaration",
                runtime
                    .bind_type_declaration(owner, &invalid, &[])
                    .map(|_| ()),
            ),
            (
                "enum",
                runtime.bind_enum_type(owner, "missing::", &[]).map(|_| ()),
            ),
            (
                "enum declaration",
                runtime
                    .bind_enum_type_declaration(owner, &invalid, &[])
                    .map(|_| ()),
            ),
            (
                "function",
                runtime
                    .bind_function::<(), ()>(owner, "missing::")
                    .map(|_| ()),
            ),
            (
                "async function",
                runtime
                    .bind_async_function::<(), ()>(owner, "missing::")
                    .map(|_| ()),
            ),
            (
                "function declaration",
                runtime
                    .bind_function_application_declaration::<(), ()>(owner, &invalid, &[])
                    .map(|_| ()),
            ),
            (
                "native payload",
                runtime
                    .bind_native_type_declaration::<i32>(owner, native, &definitions, &[])
                    .map(|_| ()),
            ),
            (
                "Rust mapping",
                runtime.bind_rust_type::<i32>(owner).map(|_| ()),
            ),
            (
                "map constructor",
                runtime
                    .bind_map_constructor::<i32, i32>(owner, argument, argument)
                    .map(|_| ()),
            ),
            (
                "set constructor",
                runtime
                    .bind_set_constructor::<i32>(owner, argument)
                    .map(|_| ()),
            ),
        ];
        for (name, result) in results {
            assert_eq!(
                result.map_err(|error| error.kind()),
                Err(RuntimeErrorKind::StaleHandle),
                "{name}"
            );
        }
        assert!(
            runtime
                .definition_context()
                .snapshot()
                .lookup(&native_path)
                .is_none()
        );
    }

    #[test]
    fn live_navigation_preserves_canonical_prefixes_and_installation_authority() {
        let mut runtime = Runtime::default();
        let input = navigation_program(false);
        let first = runtime.load_program("same-label", input.clone()).unwrap();
        let second = runtime.load_program("same-label", input).unwrap();
        let prefix = first.module("api").unwrap();
        let alias = first.module("alias").unwrap();
        assert_eq!(prefix.namespace, alias.namespace);
        assert!(Arc::ptr_eq(&prefix.installation, &alias.installation));
        let users = prefix.module("users").unwrap();
        assert_eq!(users.module("parent").unwrap().namespace, prefix.namespace);
        assert_eq!(
            first.module("internal").unwrap_err().kind(),
            RuntimeErrorKind::TypedPathValidation
        );
        assert_eq!(
            users.module("added").unwrap_err().kind(),
            RuntimeErrorKind::TypedPathValidation
        );
        let old = runtime.select_program(&first).unwrap();
        assert_eq!(old.members().count(), 1);
        let argument = runtime
            .resolve_type_arguments(&old, &[Ty::Builtin(BuiltinType::I32)])
            .unwrap()
            .pop()
            .unwrap();
        let candidate = runtime
            .stage_reload_program(&first, Some("new-label".into()), navigation_program(true))
            .unwrap();
        runtime.publish_staged_reload(candidate).unwrap();
        let added = users.module("added").unwrap();
        assert_eq!(added.namespace, first.root().namespace);
        assert!(Arc::ptr_eq(&added.installation, &first.installation));
        assert!(prefix.module("users::added").is_ok());
        assert!(alias.module("users::added").is_ok());
        assert!(second.module("api::users::added").is_err());
        assert_ne!(runtime.select_program(&first).unwrap().key(), old.key());

        let foreign = Runtime::default();
        assert_live_binding_authority(&foreign, &prefix, &argument);
        foreign.close().unwrap();
        assert_live_binding_authority(&foreign, &prefix, &argument);
        runtime.close().unwrap();
        assert_live_binding_authority(&runtime, &prefix, &argument);
        assert_eq!(
            first.root().module("api").unwrap_err().kind(),
            RuntimeErrorKind::StaleHandle
        );
        assert_eq!(
            users.module("added").unwrap_err().kind(),
            RuntimeErrorKind::StaleHandle
        );
        assert!(runtime.validate_executable_module(&old).is_err());
    }
}

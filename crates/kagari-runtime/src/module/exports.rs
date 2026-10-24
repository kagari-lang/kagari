//! Live public namespace navigation and exact executable export evidence.

use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    module::{ExecutableModule, Module, ModuleSnapshot, Program, VerifiedProgram},
};
use kagari_bytecode::program::exports::ExportTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{
    exports::{
        ProgramExports,
        path::{PublicPathError, PublicPathKind, PublicPathTarget},
    },
    ids::FunctionRef,
    types::named::NamedContract,
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
        let target = snapshot.resolve_public_path(path, PublicPathKind::Module)?;
        let PublicPathTarget::Module(namespace) = target else {
            return Err(public_path_error(
                path,
                PublicPathKind::Module,
                PublicPathError::InvalidGraph,
            ));
        };
        Ok(self.namespace_handle(namespace.clone()))
    }
}

impl ModuleSnapshot {
    /// Resolve checked public evidence relative to this captured namespace.
    /// Runtime binders must capture through `Runtime::capture_module` first; the
    /// returned evidence borrows this publication and never grants a private Module.
    pub(crate) fn resolve_public_path(
        &self,
        path: &str,
        kind: PublicPathKind,
    ) -> Result<PublicPathTarget<'_>, RuntimeError> {
        self.root
            .program
            .code
            .export_graph()
            .resolve_public_path(&self.namespace, path, kind)
            .map_err(|error| public_path_error(path, kind, error))
    }

    /// Resolve a public function and its checked executable slot in one publication.
    /// Unspecialized generic declarations remain valid names without call authority.
    pub(crate) fn function_target(
        &self,
        path: &str,
    ) -> Result<(ExecutableModule, FunctionRef), RuntimeError> {
        let invalid = || {
            public_path_error(
                path,
                PublicPathKind::Function,
                PublicPathError::InvalidGraph,
            )
        };
        let PublicPathTarget::Function(declaration) =
            self.resolve_public_path(path, PublicPathKind::Function)?
        else {
            return Err(invalid());
        };
        let code = &self.root.program.code;
        let Some(target) = code.export_targets.get(&declaration) else {
            if code.modules.iter().any(|module| {
                module.named_declarations.iter().any(|record| {
                    record.declaration == declaration
                        && matches!(&record.contract, NamedContract::Function(function)
                            if !function.generic_params.is_empty())
                })
            }) {
                return Err(RuntimeError::module_validation(format!(
                    "missing executable function application: {path:?} (expected function)"
                )));
            }
            return Err(invalid());
        };
        let owner = self.root.member(target.module).ok_or_else(invalid)?;
        owner
            .bytecode
            .functions
            .get(target.function.index())
            .ok_or_else(invalid)?;
        Ok((owner, target.function))
    }
}

fn public_path_error(path: &str, kind: PublicPathKind, error: PublicPathError) -> RuntimeError {
    let expected = match kind {
        PublicPathKind::Module => "module",
        PublicPathKind::Function => "function",
        PublicPathKind::Type => "type",
    };
    let detail = match error {
        PublicPathError::InvalidPath => "invalid public path",
        PublicPathError::NotFound => "public path not found",
        PublicPathError::WrongTargetKind => "public path has wrong target kind",
        PublicPathError::InvalidGraph => {
            return RuntimeError::new(
                RuntimeErrorKind::EngineFault,
                format!("invalid verified public target: {path:?} (expected {expected})"),
            );
        }
    };
    RuntimeError::typed_path_validation(format!("{detail}: {path:?} (expected {expected})"))
}

impl ExecutableModule {
    /// Resolve a public authored function declaration, including unspecialized generics.
    /// The ID belongs to `definitions()`; it grants neither an executable entry nor
    /// private access. This intentional Program-root query supports source updates
    /// without requiring an emitted specialization slot; live Module binding is relative.
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
        Runtime,
        error::RuntimeErrorKind,
        frame::types::arguments::TypeArgument,
        module::{Module, VerifiedProgram},
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
    use kagari_compiler::{
        bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir,
    };
    use kagari_contract::exports::{
        ExportNamespace, ProgramExports,
        path::{PublicPathKind, PublicPathTarget},
    };
    use kagari_hir::analysis::AnalysisDatabase;
    use kagari_source::source_database::{SourceDatabase, SourceLayer};
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    use rstest::rstest;
    use std::sync::Arc;

    const FUNCTION_PATHS: &str = r#"
        pub fn run() -> i32 { 1 }
        pub fn api() -> i32 { 3 }
        pub type Count = i32;
        mod hidden {
            pub fn left() -> i32 { 10 }
            pub fn right() -> i32 { 20 }
            fn secret() -> i32 { 0 }
        }
        pub mod api {
            pub fn run() -> i32 { 2 }
            pub use crate::hidden::left as selected;
            pub use crate as again;
            pub fn generic<T>(value: T) -> T { value }
        }
        pub use crate::api as mirror;
        fn main() {}
    "#;

    fn function_program(source: &str) -> VerifiedProgram {
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("paths.kgr", source.into(), SourceLayer::Base)
            .unwrap();
        let mut analysis = AnalysisDatabase::default();
        let checked = analysis
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap()
            .check_program(root, &Default::default())
            .unwrap();
        let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
        let bytecode = lower_program_to_bytecode(&mir, &Default::default()).unwrap();
        VerifiedProgram::new(
            bytecode.into_unverified(),
            &DefinitionContext::new().unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn relative_function_bindings_keep_declarations_across_live_alias_retarget() {
        let mut runtime = Runtime::default();
        let program = runtime
            .load_verified_program("paths", function_program(FUNCTION_PATHS))
            .unwrap();
        let root = program.root();
        let child = program.module("api").unwrap();
        let alias = program.module("mirror").unwrap();
        let root_run = runtime.bind_function::<(), i32>(&root, "run").unwrap();
        let child_run = runtime.bind_function::<(), i32>(&child, "run").unwrap();
        let nested = runtime.bind_function::<(), i32>(&root, "api::run").unwrap();
        let cycled = runtime
            .bind_function::<(), i32>(&alias, "again::api::run")
            .unwrap();
        assert_ne!(
            root_run.call_route().unwrap().declaration(),
            child_run.call_route().unwrap().declaration()
        );
        assert_eq!(
            nested.call_route().unwrap().declaration(),
            child_run.call_route().unwrap().declaration()
        );
        assert_eq!(
            cycled.call_route().unwrap().declaration(),
            child_run.call_route().unwrap().declaration()
        );
        // The function named api must not shadow the module of the same name.
        runtime.bind_function::<(), i32>(&root, "api").unwrap();
        let retained = runtime
            .bind_function::<(), i32>(&child, "selected")
            .unwrap();
        let captured = runtime.capture_module(&child).unwrap();
        let old_target = captured.function_target("selected").unwrap();
        assert_eq!(old_target.0.bytecode.identity.path, ["hidden"]);
        assert!(root.module("hidden").is_err());
        assert!(child.module("selected").is_err());
        let replacement = FUNCTION_PATHS
            .replace("hidden::left as selected", "hidden::right as selected")
            .replace("{ 10 }", "{ 11 }");
        let candidate = runtime
            .stage_reload_verified_program(&program, None, function_program(&replacement))
            .unwrap();
        runtime.publish_staged_reload(candidate).unwrap();
        let current = runtime.select_program(&program).unwrap();
        let fresh = runtime
            .bind_function::<(), i32>(&alias, "selected")
            .unwrap();
        assert_ne!(
            retained.call_route().unwrap().declaration(),
            fresh.call_route().unwrap().declaration()
        );
        assert_eq!(
            captured.function_target("selected").unwrap().0.key(),
            old_target.0.key()
        );
        assert_ne!(
            current.publication_token(),
            old_target.0.publication_token()
        );
        for function in [&retained, &fresh, &child_run] {
            assert_eq!(
                function
                    .prepare_call(&runtime)
                    .unwrap()
                    .owner()
                    .publication_token(),
                current.publication_token()
            );
        }
        let rebound = runtime.bind_function::<(), i32>(&child, "run").unwrap();
        assert_eq!(
            child_run.call_route().unwrap().declaration(),
            rebound.call_route().unwrap().declaration()
        );
    }

    #[test]
    fn public_binding_errors_preserve_syntax_kind_and_generic_declaration_evidence() {
        let mut runtime = Runtime::default();
        let program = runtime
            .load_verified_program("paths", function_program(FUNCTION_PATHS))
            .unwrap();
        let root = program.root();
        let child = program.module("api").unwrap();
        // No root fallback even though Count exists there.
        let error = runtime
            .bind_function::<(), i32>(&child, "Count")
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "typed path validation failed: public path not found: \"Count\" (expected function)"
        );
        let snapshot = runtime.capture_module(&root).unwrap();
        assert!(matches!(
            snapshot.resolve_public_path("Count", PublicPathKind::Type),
            Ok(PublicPathTarget::Type(_))
        ));
        let selected = runtime.select_program(&program).unwrap();
        let generic = selected
            .exported_function_declaration("api::generic")
            .unwrap();
        assert!(
            matches!(runtime.capture_module(&child).unwrap().resolve_public_path("generic", PublicPathKind::Function), Ok(PublicPathTarget::Function(id)) if id == generic)
        );
        for result in [
            runtime
                .bind_function::<(i32,), i32>(&child, "generic")
                .map(|_| ()),
            runtime
                .bind_async_function::<(i32,), i32>(&child, "generic")
                .map(|_| ()),
        ] {
            let error = result.unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
            assert_eq!(
                error.to_string(),
                "module validation failed: missing executable function application: \"generic\" (expected function)"
            );
        }
        assert!(selected.entry_function().is_some());
        let mut invalid = runtime.capture_module(&root).unwrap();
        invalid.namespace = ModuleIdentity::single_file("absent-namespace");
        assert_eq!(
            invalid
                .resolve_public_path("run", PublicPathKind::Function)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::EngineFault
        );
        assert_eq!(runtime.gc().allocated_objects(), 0);
        assert_eq!(runtime.gc().active_roots(), 0);
    }

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

    #[rstest]
    #[case::invalid_segment("absent::bad-name", "invalid public path")]
    #[case::empty_segment("run::", "invalid public path")]
    #[case::missing("missing", "public path not found")]
    #[case::private("hidden::secret", "public path not found")]
    #[case::entry_not_export("main", "public path not found")]
    #[case::type_not_function("Count", "public path has wrong target kind")]
    #[case::module_not_function("api::again", "public path has wrong target kind")]
    fn public_function_binding_errors_preserve_exact_path_diagnostics(
        #[case] path: &str,
        #[case] detail: &str,
    ) {
        let mut runtime = Runtime::default();
        let program = runtime
            .load_verified_program("paths", function_program(FUNCTION_PATHS))
            .unwrap();
        let root = program.root();
        for result in [
            runtime.bind_function::<(), i32>(&root, path).map(|_| ()),
            runtime
                .bind_async_function::<(), i32>(&root, path)
                .map(|_| ()),
        ] {
            let error = result.expect_err(path);
            assert_eq!(error.kind(), RuntimeErrorKind::TypedPathValidation);
            assert_eq!(
                error.to_string(),
                format!("typed path validation failed: {detail}: {path:?} (expected function)")
            );
        }
    }

    #[rstest]
    #[case::module(PublicPathKind::Module)]
    #[case::type_leaf(PublicPathKind::Type)]
    fn public_snapshot_rejects_function_as_module_or_type(#[case] kind: PublicPathKind) {
        let mut runtime = Runtime::default();
        let program = runtime
            .load_verified_program("paths", function_program(FUNCTION_PATHS))
            .unwrap();
        let snapshot = runtime.capture_module(&program.root()).unwrap();
        let expected = if kind == PublicPathKind::Module {
            "module"
        } else {
            "type"
        };
        let error = snapshot.resolve_public_path("run", kind).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::TypedPathValidation);
        assert_eq!(
            error.to_string(),
            format!(
                "typed path validation failed: public path has wrong target kind: \"run\" (expected {expected})"
            )
        );
    }
}

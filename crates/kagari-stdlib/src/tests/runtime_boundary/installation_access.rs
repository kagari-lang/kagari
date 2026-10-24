use super::support;

use crate::{declarations::StandardDeclarations, namespaces};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata},
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::types::{PublicItem, TraitContract};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    host::HostCallContext,
    module::VerifiedProgram,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        context::CallContext,
        declarations::{FunctionDecl, MethodDecl},
        module::NativeModule,
        types::Type,
    },
    reload::ReloadValidationError,
    session::ExecutionOptions,
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::declaration::{NativeDeclaration, TraitDef, host::HostCallPolicy};
use support::native::{
    invoke::invoke,
    program::{ProgramFixture, free_function_program},
};

#[test]
fn installed_function_needs_no_execution_permissions() {
    let mut builder = ModuleBuilder::new("host", &Default::default()).unwrap();
    let function = builder
        .define_function(
            FunctionDecl::new("answer")
                .returns(Type::i32())
                .host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    builder
        .bind(function, |_: &HostCallContext<'_>, _: &[Value]| {
            Ok(Value::I32(42))
        })
        .unwrap();
    let module = builder.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = Runtime::default();
    assert!(
        runtime
            .load_program("uninstalled", program.clone())
            .is_err()
    );
    assert_eq!(runtime.modules().loaded_count(), 0);
    module.install(&mut runtime).unwrap();
    let loaded = runtime.load_program("installed", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    assert_eq!(invoke(&runtime, &loaded, 0, &[]).unwrap(), Value::I32(42));
}

#[test]
fn host_cancellation_is_sticky_even_when_the_handler_returns_success() {
    let cancel = CancellationToken::default();
    let callback_cancel = cancel.clone();
    let mut builder = ModuleBuilder::new("host", &Default::default()).unwrap();
    let function = builder
        .define_function(
            FunctionDecl::new("cancel")
                .returns(Type::i32())
                .host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    builder
        .bind(function, move |_: &HostCallContext<'_>, _: &[Value]| {
            callback_cancel.cancel();
            Ok(Value::I32(42))
        })
        .unwrap();
    let module = builder.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let loaded = runtime.load_program("cancel", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let session = runtime
        .begin_execution(
            &loaded,
            ExecutionOptions {
                cancellation: cancel,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        invoke(&runtime, &loaded, 0, &[]).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(
        runtime.resources().poll_execution().unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(runtime.gc().active_roots(), 0);
    drop(session);
    let _next = runtime
        .begin_execution(&loaded, ExecutionOptions::default())
        .unwrap();
    runtime.resources().poll_execution().unwrap();
}

#[test]
fn ordinary_installed_traits_require_exact_contracts_without_calls() {
    let context = DefinitionContext::new().unwrap();
    let mut builder = ModuleBuilder::new(
        "example::plain",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut interface = builder.define_trait("Read").unwrap();
    interface
        .define_method(MethodDecl::instance("read").returns(Type::i32()))
        .unwrap();
    interface.finish().unwrap();
    let installed = builder.finish().unwrap();
    let declaration = installed
        .to_declaration()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let contract = declaration.traits[0].clone();
    let id = declaration
        .definition(DefinitionKind::Trait, &contract.name, &context.snapshot())
        .unwrap();
    let program = |contract: TraitDef, private: bool| {
        let mut module = BytecodeModule {
            identity: declaration.identity.clone(),
            ..Default::default()
        };
        if private {
            module.trait_contracts.push(TraitContract {
                declaration: id,
                abi: contract,
            });
        } else {
            module.public_items.push(PublicItem::Trait(contract));
        }
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![module],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap()
    };
    // A plain declaration alone has no native privilege or installation dependency.
    let mut runtime = Runtime::default();
    runtime
        .load_program("plain", program(contract.clone(), false))
        .unwrap();
    installed.install(&mut runtime).unwrap();
    let active = runtime
        .load_program("registered", program(contract.clone(), false))
        .unwrap();
    let _active_program = active;
    runtime
        .load_program("private", program(contract.clone(), true))
        .unwrap();
    let mut forged = contract;
    forged.methods[0].params[0].name = "forged_receiver".into();
    for private in [false, true] {
        let candidate = program(forged.clone(), private);
        VerifiedProgram::new(candidate.clone(), &context).unwrap();
        assert!(runtime.load_program("forged", candidate.clone()).is_err());
        assert!(
            runtime
                .stage_reload_program(&_active_program, Some("registered".into()), candidate)
                .is_err()
        );
    }
}

#[test]
fn native_declarations_require_exact_installation_without_calls() {
    let context = DefinitionContext::new().unwrap();
    let mut builder = ModuleBuilder::new(
        "example::unused",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let function = builder
        .define_function(FunctionDecl::new("answer").returns(Type::i32()))
        .unwrap();
    builder
        .bind(function, |_: &mut CallContext<'_>| -> NativeResult<i32> {
            panic!("declaration validation must not execute native code")
        })
        .unwrap();
    let installed = builder.finish().unwrap();
    let declaration = installed
        .to_declaration()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let function = declaration.functions[0].clone();
    let id = declaration
        .definition(
            DefinitionKind::Function,
            &function.name,
            &context.snapshot(),
        )
        .unwrap();
    let contract = NativeDeclaration {
        concrete_result: None,
        declaration: id,
        function,
        callable_requirements: vec![],
    };
    let program = |contract: NativeDeclaration| {
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                identity: declaration.identity.clone(),
                native_declarations: vec![contract],
                ..Default::default()
            }],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap()
    };
    let mut runtime = Runtime::default();
    VerifiedProgram::new(program(contract.clone()), &context).unwrap();
    assert!(
        runtime
            .load_program("uninstalled", program(contract.clone()))
            .is_err()
    );
    installed.install(&mut runtime).unwrap();
    let active = runtime
        .load_program("registered", program(contract.clone()))
        .unwrap();
    let _active_program = active;
    let mut forged = contract;
    forged.function.return_type = Type::bool().abi().clone();
    VerifiedProgram::new(program(forged.clone()), &context).unwrap();
    assert!(
        runtime
            .load_program("forged", program(forged.clone()))
            .is_err()
    );
    assert!(
        runtime
            .stage_reload_program(&_active_program, Some("registered".into()), program(forged))
            .is_err()
    );
}

#[test]
fn reserved_core_roles_require_exact_installed_declarations_without_calls() {
    let context = DefinitionContext::new().unwrap();
    let foundation = StandardDeclarations::default();
    // Use the normal producer for the complete default/template proof closure;
    // runtime installation and reload consume only the resulting bytecode.
    let declarations = foundation.declarations().unwrap();
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("installation.kgr", "fn main() {}".into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(declarations.to_vec());
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let compiled = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let modules = &compiled.modules;
    for (index, module) in modules
        .iter()
        .enumerate()
        .filter(|(_, module)| namespaces::is_language_module(&module.identity))
    {
        let definition = declarations
            .iter()
            .find(|definition| definition.records().identity == module.identity)
            .unwrap()
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records();
        let base = modules[index].clone();
        let program = |module| {
            // Retain the producer's complete proof/evidence closure rather than
            // extracting modules and losing authored generic/default-body records.
            let mut program = compiled.clone();

            program.modules[index] = module;
            DefinitionMetadata::checked(context.snapshot(), program, &Default::default()).unwrap()
        };
        let mut runtime = Runtime::default();
        assert!(
            runtime
                .load_program("uninstalled_core", program(base.clone()))
                .is_err()
        );
        NativeModule::install_all(&crate::modules().unwrap(), &mut runtime).unwrap();
        runtime
            .load_program("core", program(base.clone()))
            .unwrap_or_else(|error| panic!("{:?}: {error}", definition.identity));
        let missing_name = base
            .public_items
            .iter()
            .find_map(|item| match item {
                PublicItem::Trait(trait_) if !trait_.methods.is_empty() => {
                    Some(trait_.name.clone())
                }
                _ => None,
            })
            .unwrap();
        let mut missing = base.clone();
        missing.public_items.retain(
            |item| !matches!(item, PublicItem::Trait(trait_) if trait_.name == missing_name),
        );
        assert!(
            runtime
                .load_program("missing_core", program(missing))
                .is_err()
        );
        let mut duplicate = base.clone();
        duplicate.public_items.push(base.public_items[0].clone());
        assert!(
            runtime
                .load_program("duplicate_core", program(duplicate))
                .is_err()
        );
    }

    // Renaming self changes interface eligibility in the complete library closure.
    // Isolate declarations from interface uses and implementation tables so forged
    // public/private contracts pass portable verification and reach installation.
    let format_module = modules
        .iter()
        .find(|module| module.identity == namespaces::module("core", "fmt"))
        .unwrap();
    let program = |module| {
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![module],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap()
    };
    let base = BytecodeModule {
        identity: format_module.identity.clone(),
        public_items: format_module
            .public_items
            .iter()
            .filter(|item| matches!(item, PublicItem::Trait(_)))
            .cloned()
            .collect(),
        ..Default::default()
    };
    VerifiedProgram::new(program(base.clone()), &context).unwrap();
    let mut runtime = Runtime::default();
    NativeModule::install_all(&crate::modules().unwrap(), &mut runtime).unwrap();
    let active = runtime
        .load_program("public_core", program(base.clone()))
        .unwrap();
    let _active_program = active;
    let mut forged = base;
    let contract = forged
        .public_items
        .iter_mut()
        .find_map(|item| match item {
            PublicItem::Trait(contract) if contract.name == "Debug" => Some(contract),
            _ => None,
        })
        .unwrap();
    contract.methods[0].params[0].name = "forged_argument".into();
    let contract = contract.clone();
    let forged = program(forged);
    VerifiedProgram::new(forged.clone(), &context).unwrap();
    let error = runtime
        .load_program("forged_core", forged.clone())
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("reserved or native trait differs from its installed contract"),
        "{error}"
    );
    // Staging rejects the changed public ABI before installation validation.
    assert!(matches!(
        runtime.stage_reload_program(&_active_program, Some("public_core".into()), forged),
        Err(ReloadValidationError::PublicAbiFingerprintMismatch)
    ));
    let root = context.intern_root(&format_module.identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Trait, &contract.name, 0)
        .unwrap();
    let private = program(BytecodeModule {
        identity: format_module.identity.clone(),
        trait_contracts: vec![TraitContract {
            declaration,
            abi: contract,
        }],
        ..Default::default()
    });
    VerifiedProgram::new(private.clone(), &context).unwrap();
    let error = runtime.load_program("private_core", private).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("private reserved or native trait lacks an installed contract"),
        "{error}"
    );
}

use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
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
    host::HostFunction,
    module::VerifiedProgram,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        context::CallContext,
        declarations::{FunctionDecl, MethodDecl},
        module::NativeModule,
        types::Type,
    },
    session::ExecutionOptions,
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::{declarations::StandardDeclarations, namespaces};
use kagari_types::{
    declaration::{NativeDeclaration, TraitDef},
    host_interface::{HostFunctionDeclaration, value_type::HostValueType},
};
use std::collections::BTreeSet;

#[test]
fn installed_function_needs_no_execution_permissions() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    assert!(runtime.invoke_host("host.answer", &[]).is_err());
    runtime
        .register_host_function(HostFunction::new(
            {
                let declaration = HostFunctionDeclaration::new(
                    &context,
                    "host.answer",
                    vec![],
                    HostValueType::I32,
                )
                .unwrap();
                DefinitionMetadata::checked(context.snapshot(), declaration, &Default::default())
                    .unwrap()
            },
            |_, _| Ok(Value::I32(42)),
        ))
        .unwrap();
    assert_eq!(
        runtime.invoke_host("host.answer", &[]).unwrap(),
        Value::I32(42)
    );
}

#[test]
fn host_cancellation_is_sticky_even_when_the_handler_returns_success() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let cancel = CancellationToken::default();
    let callback_cancel = cancel.clone();
    runtime
        .register_host_function(HostFunction::new(
            {
                let declaration = HostFunctionDeclaration::new(
                    &context,
                    "host.cancel",
                    vec![],
                    HostValueType::I32,
                )
                .unwrap();
                DefinitionMetadata::checked(context.snapshot(), declaration, &Default::default())
                    .unwrap()
            },
            move |_, _| {
                callback_cancel.cancel();
                Ok(Value::I32(42))
            },
        ))
        .unwrap();
    let module = runtime
        .load_program(
            "cancel",
            DefinitionMetadata::checked(
                context.snapshot(),
                BytecodeProgram {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let session = runtime
        .begin_execution(
            &module,
            ExecutionOptions {
                cancellation: cancel,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        runtime.invoke_host("host.cancel", &[]).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(
        runtime.resources().poll_execution().unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    drop(session);
    let _next = runtime
        .begin_execution(&module, ExecutionOptions::default())
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
        DefinitionMetadata::checked(
            context.snapshot(),
            BytecodeProgram {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module],
            },
            &Default::default(),
        )
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
                .stage_reload_program(&active, "registered", candidate)
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
        DefinitionMetadata::checked(
            context.snapshot(),
            BytecodeProgram {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule {
                    identity: declaration.identity.clone(),
                    native_declarations: vec![contract],
                    ..Default::default()
                }],
            },
            &Default::default(),
        )
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
            .stage_reload_program(&active, "registered", program(forged))
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
    let modules = lower_program_to_bytecode(&mir)
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records()
        .modules;
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
            let mut modules = modules.clone();
            modules[index] = module;
            let mut included = BTreeSet::new();
            let mut pending = vec![index];
            while let Some(index) = pending.pop() {
                if included.insert(index) {
                    pending.extend(
                        modules[index]
                            .dependencies
                            .iter()
                            .map(|owner| owner.index()),
                    );
                }
            }
            let order = included.into_iter().collect::<Vec<_>>();
            let reference =
                |index| ModuleRef::new(order.iter().position(|owner| *owner == index).unwrap());
            DefinitionMetadata::checked(
                context.snapshot(),
                BytecodeProgram {
                    exports: Default::default(),
                    root: reference(index),
                    modules: order
                        .iter()
                        .map(|index| {
                            let mut module = modules[*index].clone();
                            module.dependencies = module
                                .dependencies
                                .iter()
                                .map(|owner| reference(owner.index()))
                                .collect();
                            module
                        })
                        .collect(),
                },
                &Default::default(),
            )
            .unwrap()
        };
        let mut runtime = Runtime::default();
        assert!(
            runtime
                .load_program("uninstalled_core", program(base.clone()))
                .is_err()
        );
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
        let active = runtime
            .load_program("core", program(base.clone()))
            .unwrap_or_else(|error| panic!("{:?}: {error}", definition.identity));
        let mut forged = base.clone();
        let changed = forged
            .public_items
            .iter_mut()
            .find_map(|item| match item {
                PublicItem::Trait(trait_) if !trait_.methods.is_empty() => Some(trait_),
                _ => None,
            })
            .unwrap();
        let changed_name = changed.name.clone();
        changed.methods[0].params[0].name = "forged_argument".into();
        // Portable structural validation accepts the declaration; installation binds
        // reserved identities to the exact checked language registration.
        VerifiedProgram::new(program(forged.clone()), &context).unwrap();
        assert!(
            runtime
                .load_program("forged_core", program(forged.clone()))
                .is_err()
        );
        assert!(
            runtime
                .stage_reload_program(&active, "core", program(forged))
                .is_err()
        );
        let mut missing = base.clone();
        missing.public_items.retain(
            |item| !matches!(item, PublicItem::Trait(trait_) if trait_.name == changed_name),
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

    // Moving a referenced public role to private correctly invalidates dependent
    // public implementation tables. Test private installation authority in an
    // independent no-call module, so rejection belongs to the installed catalog.
    let format_module = modules
        .iter()
        .find(|module| module.identity == namespaces::module("core", "fmt"))
        .unwrap();
    let mut contract = format_module
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::Trait(contract) if contract.name == "Debug" => Some(contract.clone()),
            _ => None,
        })
        .unwrap();
    let root = context.intern_root(&format_module.identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Trait, &contract.name, 0)
        .unwrap();
    contract.methods[0].params[0].name = "forged_argument".into();
    let private = DefinitionMetadata::checked(
        context.snapshot(),
        BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                identity: format_module.identity.clone(),
                trait_contracts: vec![TraitContract {
                    declaration,
                    abi: contract,
                }],
                ..Default::default()
            }],
        },
        &Default::default(),
    )
    .unwrap();
    VerifiedProgram::new(private.clone(), &context).unwrap();
    let mut runtime = Runtime::default();
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let error = runtime.load_program("private_core", private).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("private reserved or native trait lacks an installed contract"),
        "{error}"
    );
}

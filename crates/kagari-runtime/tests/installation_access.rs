use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::host_interface::{HostFunctionDeclaration, value_type::HostValueType};
use kagari_common::{cancellation::CancellationToken, collection::CollectionAccess};
use kagari_contract::types::{PublicItem, TraitContract, TraitDef};
use kagari_runtime::native::{builder::ModuleBuilder, language::LanguageContracts};
use kagari_runtime::{Runtime, host::HostFunction, value::Value};
use kagari_runtime::{error::RuntimeErrorKind, session::ExecutionOptions};

#[test]
fn installed_function_needs_no_execution_permissions() {
    let mut runtime = Runtime::default();
    assert!(runtime.invoke_host("host.answer", &[]).is_err());
    runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new("host.answer", vec![], HostValueType::I32),
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
    let mut runtime = Runtime::default();
    let cancel = CancellationToken::default();
    let callback_cancel = cancel.clone();
    runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new("host.cancel", vec![], HostValueType::I32),
            move |_, _| {
                callback_cancel.cancel();
                Ok(Value::I32(42))
            },
        ))
        .unwrap();
    let module = runtime
        .load_program(
            "cancel",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
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
fn storage_capabilities_require_exact_installation_without_native_calls() {
    let mut builder = ModuleBuilder::new("example::storage", &LanguageContracts::default());
    let mut interface = builder.define_trait("CustomStorage");
    interface.storage_view(CollectionAccess::ReadOnly);
    interface.finish().unwrap();
    let installed = builder.finish().unwrap();
    let declaration = installed.to_declaration().unwrap();
    let contract = declaration.traits[0].clone();
    let program = |contract: TraitDef, private: bool| {
        let mut module = BytecodeModule {
            identity: declaration.identity.clone(),
            ..Default::default()
        };
        if private {
            module.trait_contracts.push(TraitContract {
                declaration: declaration.definition(
                    kagari_common::identity::DefinitionKind::Trait,
                    &contract.name,
                ),
                abi: contract,
            });
        } else {
            module.public_items.push(PublicItem::Trait(contract));
        }
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![module],
        }
    };
    let mut runtime = Runtime::default();
    assert!(
        runtime
            .load_program("uninstalled", program(contract.clone(), false))
            .is_err()
    );
    installed.install(&mut runtime).unwrap();
    let active = runtime
        .load_program("storage", program(contract.clone(), false))
        .unwrap();
    let mut forged = contract.clone();
    forged.storage_access = Some(CollectionAccess::Mutable);
    assert!(
        runtime
            .load_program("forged", program(forged.clone(), false))
            .is_err()
    );
    assert!(
        runtime
            .stage_reload_program(&active, "storage", program(forged, false))
            .is_err()
    );
    // An arbitrary private trait cannot acquire native storage capabilities.
    let mut private = contract;
    private.name = "UninstalledPrivate".into();
    assert!(
        runtime
            .load_program("private", program(private, true))
            .is_err()
    );
}

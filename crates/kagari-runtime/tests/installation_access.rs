use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};
use kagari_contract::{
    layout::{EnumLayout, EnumVariantLayout},
    types::{PublicItem, TraitContract},
};
use kagari_runtime::{
    Runtime, error::RuntimeErrorKind, host::HostFunction, module::VerifiedProgram,
    native::builder::ModuleBuilder, session::ExecutionOptions, value::Value,
};
use kagari_stdlib::{declarations::StandardDeclarations, namespaces};
use kagari_types::{
    collection::CollectionAccess,
    declaration::{TraitDef, TypeDefKind, module::ModuleDecl},
    host_interface::{HostFunctionDeclaration, value_type::HostValueType},
    language::Protocol,
};
use std::collections::BTreeSet;

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
    let mut builder = ModuleBuilder::new(
        "example::storage",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
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

#[test]
fn reserved_core_roles_require_exact_installed_declarations_without_calls() {
    let foundation = StandardDeclarations::default();
    // Nominal carrier outputs must travel with their declaration owners even
    // when the program has no calls. Use the registered dependency closure.
    let declarations = foundation.declarations();
    let modules = declarations
        .iter()
        .map(|definition| BytecodeModule {
            identity: definition.identity.clone(),
            enumerations: definition
                .types
                .iter()
                .filter(|ty| ty.kind == TypeDefKind::Enum)
                .map(|ty| {
                    let declaration = definition.definition(DefinitionKind::Enum, &ty.name);
                    EnumLayout {
                        declaration: declaration.clone(),
                        arguments: ty
                            .generic_params
                            .iter()
                            .map(|parameter| parameter.as_type())
                            .collect(),
                        variants: ty
                            .variants
                            .iter()
                            .map(|variant| EnumVariantLayout {
                                declaration: ModuleDecl::variant_id(&declaration, &variant.name),
                                payload: variant.payload.clone(),
                                reports_failure: variant.reports_failure,
                            })
                            .collect(),
                    }
                })
                .collect(),
            dependencies: definition
                .dependencies
                .iter()
                .map(|dependency| {
                    ModuleRef::new(
                        declarations
                            .iter()
                            .position(|owner| &owner.identity == dependency)
                            .unwrap(),
                    )
                })
                .collect(),
            public_items: definition
                .traits
                .iter()
                .filter(|trait_| {
                    Protocol::from_id(&definition.definition(DefinitionKind::Trait, &trait_.name))
                        .is_some()
                })
                .cloned()
                .map(PublicItem::Trait)
                .chain(definition.types.iter().cloned().map(PublicItem::Type))
                .collect(),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    for (index, definition) in declarations
        .iter()
        .enumerate()
        .filter(|(_, module)| namespaces::is_language_module(&module.identity))
    {
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
            BytecodeProgram {
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
            }
        };
        let mut runtime = Runtime::default();
        assert!(
            runtime
                .load_program("uninstalled_core", program(base.clone()))
                .is_err()
        );
        kagari_runtime::native::module::NativeModule::install_all(
            &kagari_stdlib::modules().unwrap(),
            &mut runtime,
        )
        .unwrap();
        let active = runtime.load_program("core", program(base.clone())).unwrap();
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
        changed.methods[0].name = "forged_method".into();
        // Portable structural validation accepts the declaration; installation binds
        // reserved identities to the exact checked language registration.
        VerifiedProgram::new(program(forged.clone())).unwrap();
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
        let mut private = base.clone();
        private.public_items.retain(
            |item| !matches!(item, PublicItem::Trait(trait_) if trait_.name == changed_name),
        );
        let mut contract = definition
            .traits
            .iter()
            .find(|trait_| trait_.name == changed_name)
            .unwrap()
            .clone();
        contract.methods[0].name = "forged_debug".into();
        private.trait_contracts.push(TraitContract {
            declaration: definition.definition(DefinitionKind::Trait, &changed_name),
            abi: contract,
        });
        VerifiedProgram::new(program(private.clone())).unwrap();
        assert!(
            runtime
                .load_program("private_core", program(private))
                .is_err()
        );
    }
}

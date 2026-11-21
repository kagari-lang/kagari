//! The production recipe publishes complete task contracts without preparing adapters.
use crate::{StandardLibrary, namespaces, test_library};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_runtime::{
    Runtime,
    native::{catalog::DeclarationCatalog, module::NativeModule, types::Type},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        TypeDefKind,
        module::ModuleDecl,
        names::{ExportName, NameNamespace},
        native::NativeStorageLayout,
    },
    ty::Ty,
};

#[test]
fn production_semantic_projection_retains_unprepared_task_owners() {
    let missing = StandardLibrary::new(&DeclarationCatalog::default())
        .err()
        .unwrap();
    assert!(
        missing
            .to_string()
            .contains("missing verified standard owner")
    );
    let seed = test_library::seed();
    let library = StandardLibrary::new(&seed).unwrap();
    let standard = library.declarations();
    let bundle = &library.bundle;
    for owner in [
        "cmp", "convert", "default", "ops", "option", "result", "fmt", "hash", "str", "iter",
    ]
    .map(|path| namespaces::module("core", path))
    .into_iter()
    .chain([namespaces::type_owner("String")])
    {
        assert!(seed.authored_origin(&owner).is_some());
        assert_eq!(
            standard.catalog().authored_origin(&owner),
            seed.authored_origin(&owner)
        );
        assert!(
            !standard
                .declarations()
                .iter()
                .any(|record| record.records().identity == owner)
        );
    }
    let option = standard.enumeration("Option").unwrap();
    for (owner, name, target) in [
        (
            namespaces::module("std", "option"),
            ExportName::new(NameNamespace::Type, "Option"),
            option.id(),
        ),
        (
            namespaces::prelude(),
            ExportName::new(NameNamespace::Value, "Some"),
            option.variant("Some").unwrap().id(),
        ),
    ] {
        let facade = standard
            .declarations()
            .iter()
            .find(|m| m.records().identity == owner)
            .unwrap();
        assert_eq!(facade.records().exports[&name], target);
        assert!(
            facade
                .records()
                .dependencies
                .contains(&namespaces::module("core", "option"))
        );
    }
    standard.ordering().unwrap();
    standard.ord().method("cmp").unwrap();
    assert!(bundle.prepared.get().is_none());
    assert!(bundle.collect().unwrap().pending.lock().unwrap().is_some());
    let snapshot = bundle
        .collect()
        .unwrap()
        .pending
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .declarations();
    let providers = snapshot.modules();
    let task = providers
        .iter()
        .find(|module| module.records().identity == namespaces::module("core", "task"))
        .unwrap();
    let future = providers
        .iter()
        .find(|module| module.records().identity == namespaces::module("core", "future"))
        .unwrap();
    let definitions = task.definitions();
    for (module, name, layout, arity) in [
        (task, "Task", NativeStorageLayout::Task, 1),
        (task, "TaskScope", NativeStorageLayout::TaskScope, 0),
        (future, "Future", NativeStorageLayout::Future, 1),
    ] {
        let ty = module
            .records()
            .types
            .iter()
            .find(|ty| ty.name == name)
            .unwrap();
        assert_eq!(ty.kind, TypeDefKind::NativeStorage(layout));
        assert_eq!(ty.generic_params.len(), arity);
        assert!(ty.bounds.is_empty());
        assert!(ty.fields.is_empty());
        assert!(ty.variants.is_empty());
    }
    let error = task
        .records()
        .types
        .iter()
        .find(|ty| ty.name == "SpawnError")
        .unwrap();
    assert_eq!(error.kind, TypeDefKind::Enum);
    assert_eq!(
        error
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        ["ScopeClosed", "CapacityExceeded", "DispatchUnavailable"]
    );
    assert!(
        error
            .variants
            .iter()
            .all(|variant| variant.payload.is_empty() && !variant.reports_failure)
    );
    assert_eq!(task.records().implementations.len(), 2);
    for (index, implementation) in task.records().implementations.iter().enumerate() {
        assert!(implementation.trait_type.is_none());
        assert!(implementation.bounds.is_empty());
        let Ty::NativeObject(receiver) = &implementation.for_type else {
            panic!("task storage receiver")
        };
        let receiver_name = definitions
            .resolve(receiver.declaration)
            .unwrap()
            .last()
            .unwrap()
            .name;
        let [method] = implementation.methods.as_slice() else {
            panic!("one inherent method per task receiver")
        };
        let owner = task
            .records()
            .implementation_id(index, definitions)
            .unwrap();
        let member = ModuleDecl::method_id(owner, &method.name, definitions).unwrap();
        assert_eq!(
            method.implementation,
            CallableImplementation::Native(member)
        );
        assert_eq!(
            definitions.resolve(member).unwrap().module(),
            &namespaces::module("core", "task")
        );
        assert!(task.records().documentation[&member].contains("```kgr"));
        assert_eq!(method.params[0].ty, implementation.for_type);
        match receiver_name {
            "TaskScope" => {
                assert_eq!(method.name, "spawn");
                assert!(implementation.generic_params.is_empty());
                let [output] = method.generic_params.as_slice() else {
                    panic!("method-local spawn output")
                };
                assert_eq!(output.owner, member);
                assert_eq!(method.params.len(), 2);
                let Ty::Function { params, result } = &method.params[1].ty else {
                    panic!("ordinary callable coercion")
                };
                assert!(params.is_empty());
                let Ty::NativeObject(result) = result.as_ref() else {
                    panic!("Future output")
                };
                assert_eq!(
                    result.declaration,
                    future
                        .records()
                        .definition(DefinitionKind::AssociatedType, "Future", definitions)
                        .unwrap()
                );
                assert_eq!(result.arguments, [output.as_type()]);
                let Ty::Enum(result) = &method.return_type else {
                    panic!("admission Result")
                };
                assert_eq!(
                    definitions.resolve(result.declaration).unwrap().module(),
                    &namespaces::module("core", "result")
                );
                assert_eq!(
                    definitions
                        .resolve(result.declaration)
                        .unwrap()
                        .last()
                        .unwrap()
                        .name,
                    "Result"
                );
                let [Ty::NativeObject(task_result), Ty::Enum(error_result)] =
                    result.arguments.as_slice()
                else {
                    panic!("Task and SpawnError")
                };
                assert_eq!(
                    task_result.declaration,
                    task.records()
                        .definition(DefinitionKind::AssociatedType, "Task", definitions)
                        .unwrap()
                );
                assert_eq!(task_result.arguments, [output.as_type()]);
                assert_eq!(
                    error_result.declaration,
                    task.records()
                        .definition(DefinitionKind::Enum, "SpawnError", definitions)
                        .unwrap()
                );
            }
            "Task" => {
                assert_eq!(method.name, "cancel");
                let [parameter] = implementation.generic_params.as_slice() else {
                    panic!("generic Task receiver")
                };
                assert_eq!(parameter.owner, owner);
                assert_eq!(receiver.arguments, [parameter.as_type()]);
                assert_eq!(method.generic_params, implementation.generic_params);
                assert_eq!(method.params.len(), 1);
                assert_eq!(method.return_type, *Type::unit().abi());
            }
            name => panic!("unexpected task receiver {name}"),
        }
    }
    // Both public declaration consumers are projections, not independent producers.
    let context = DefinitionContext::new().unwrap();
    for owner in [
        namespaces::module("core", "task"),
        namespaces::module("core", "future"),
        namespaces::prelude(),
    ] {
        let local = providers
            .iter()
            .find(|module| module.records().identity == owner)
            .unwrap();
        let local = local.import_into(&context, &Default::default()).unwrap();
        let other = standard
            .declarations()
            .iter()
            .find(|module| module.records().identity == owner)
            .unwrap();
        let other = other.import_into(&context, &Default::default()).unwrap();
        assert_eq!(
            local
                .records()
                .compiler_input_bytes(local.definitions(), &Default::default())
                .unwrap(),
            other
                .records()
                .compiler_input_bytes(other.definitions(), &Default::default())
                .unwrap()
        );
        assert_eq!(local.records().documentation, other.records().documentation);
        assert_eq!(
            local.records().module_documentation,
            other.records().module_documentation
        );
    }
    assert!(bundle.prepared.get().is_none());
    assert!(bundle.collect().unwrap().pending.lock().unwrap().is_some());
    let modules = library.modules().unwrap();
    NativeModule::install_all(&modules, &mut Runtime::default()).unwrap();
    assert!(bundle.collect().unwrap().pending.lock().unwrap().is_none());
    for declaration in [task, future] {
        let prepared = modules
            .iter()
            .find(|module| module.declaration().identity == declaration.records().identity)
            .unwrap();
        assert_eq!(
            prepared.declaration().implementations,
            declaration.records().implementations
        );
        assert_eq!(
            prepared.declaration().documentation,
            declaration.records().documentation
        );
    }
}

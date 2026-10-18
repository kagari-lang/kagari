//! Free-function configuration shares binder/proof checks without early native preparation.
use crate::native::{
    assembly::{ModuleDefinition, ModuleSetBuilder, conflict, draft::TraitImplementation},
    binding::{Codec, NativeBinding, NativeResult},
    catalog::DeclarationCatalog,
    declarations::{FunctionDecl, MethodDecl, SelectedCall},
    types::Type,
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
};
use kagari_types::{declaration::TypeDefKind, ty::Ty};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

fn owner(name: &str) -> ModuleIdentity {
    ModuleIdentity::single_file(name)
}

fn contract_path() -> DefinitionPath {
    DefinitionPath {
        module: owner("contracts"),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Measure".into(),
            occurrence: 0,
        }],
    }
}

#[test]
fn configured_function_adopts_forward_facts_and_transfers_only_pending_preparation() {
    for reverse in [false, true] {
        let configurations = Rc::new(Cell::new(0));
        let count = configurations.clone();
        let factories = Arc::new(AtomicUsize::new(0));
        let prepared = factories.clone();
        let function = ModuleDefinition::new(owner("functions"), move |module| {
            let interface = module.reference_trait(contract_path(), 0)?;
            let measure = module.reference_method(&interface, "measure", 0)?;
            module.define_function(
                FunctionDecl::new("measure")
                    .configure(move |function| {
                        count.set(count.get() + 1);
                        let item = function.type_parameter("T")?.ty();
                        function.parameter("value", item.clone())?;
                        function.bound_draft(item.clone(), interface.apply([])?)?;
                        function.requires_draft(measure.requirement(
                            item,
                            interface.apply([])?,
                            [],
                        )?)?;
                        Ok(())
                    })
                    .documentation("Measure one value using its declared protocol.")
                    .returns(Type::u64())
                    .implementation_with_send(move |view| {
                        prepared.fetch_add(1, Ordering::SeqCst);
                        let declaration = view.declaration();
                        let function = &declaration.records().function;
                        assert_eq!(
                            function.generic_params[0].owner,
                            declaration.records().declaration
                        );
                        assert_eq!(function.params[0].ty, function.generic_params[0].as_type());
                        let [required] = declaration.records().callable_requirements.as_slice()
                        else {
                            panic!("one selected slot")
                        };
                        assert_eq!(required.receiver, function.params[0].ty);
                        SelectedCall::from_declaration(declaration, required)?;
                        Ok(NativeBinding::new(
                            vec![Codec::Value],
                            Codec::Exact(Type::u64()),
                            |_| panic!("preparation must not invoke callback"),
                        ))
                    }),
            )?;
            Ok(())
        });
        let contracts = ModuleDefinition::new(owner("contracts"), |module| {
            let id = module.definition(DefinitionKind::Trait, "Measure")?;
            let method =
                module.trait_method(id, MethodDecl::instance("measure").returns(Type::u64()))?;
            module.define_trait("Measure", 0, |contract, _| {
                contract.methods.push(method);
                Ok(())
            })?;
            Ok(())
        });
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        for definition in if reverse {
            [contracts, function]
        } else {
            [function, contracts]
        } {
            set.add(definition).unwrap();
        }
        let assembly = set.finish().unwrap();
        assert_eq!(configurations.get(), 1);
        assert_eq!(factories.load(Ordering::SeqCst), 0);
        let snapshot = assembly.declarations();
        let transferred = assembly.try_into_send().unwrap();
        thread::spawn(move || transferred.into_modules().unwrap())
            .join()
            .unwrap();
        assert_eq!(factories.load(Ordering::SeqCst), 1);
        assert_eq!(snapshot.modules().len(), 2);
    }
}

#[test]
fn failed_configuration_does_not_publish_facts_and_same_name_can_be_retried() {
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner("atomic"), |module| {
        assert!(
            module
                .define_function(
                    FunctionDecl::new("create")
                        .documentation("must not survive")
                        .configure(|function| {
                            function.type_parameter("T")?;
                            function.produces(Type::bool())?;
                            Err(conflict("configuration rejected"))
                        })
                        .implementation_with(|_| -> NativeResult<NativeBinding> {
                            panic!("failed submission retained factory")
                        })
                )
                .is_err()
        );
        module.define_function(
            FunctionDecl::new("create").implementation(NativeBinding::new(
                vec![],
                Codec::Exact(Type::unit()),
                |_| panic!("not executed"),
            )),
        )?;
        Ok(())
    }))
    .unwrap();
    let assembly = set.finish().unwrap();
    let snapshot = assembly.declarations();
    let module = snapshot.modules()[0].records();
    assert_eq!(module.functions.len(), 1);
    assert!(module.functions[0].generic_params.is_empty());
    assert!(module.concrete_results.is_empty());
    assert!(module.callable_requirements.is_empty());
    assert!(module.documentation.is_empty());
    assembly.into_modules().unwrap();
}

#[test]
fn free_function_concrete_results_and_selected_calls_keep_required_proofs() {
    for missing_bound in [false, true] {
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        set.add(ModuleDefinition::new(owner("proofs"), move |module| {
            let id = module.definition(DefinitionKind::Trait, "Measure")?;
            let method =
                module.trait_method(id, MethodDecl::instance("measure").returns(Type::u64()))?;
            let measure = module.define_trait("Measure", 0, |record, _| {
                record.methods.push(method);
                Ok(())
            })?;
            let selected = module.reference_method(&measure, "measure", 0)?;
            let node = module.define_type("Node", TypeDefKind::Struct, 0, |_, _| Ok(()))?;
            let view = module.define_trait("View", 0, |_, _| Ok(()))?;
            module.trait_implementation_with(0, |_| {
                Ok(TraitImplementation {
                    receiver: node.apply([])?,
                    interface: view.apply([])?,
                    bounds: vec![],
                    bindings: vec![],
                })
            })?;
            module.define_function(
                FunctionDecl::new("project")
                    .returns(view.apply([])?)
                    .produces(node.apply([])?)
                    .configure(move |function| {
                        let parameter = function.type_parameter("T")?.ty();
                        function.parameter("source", parameter.clone())?;
                        if !missing_bound {
                            function.bound_draft(parameter.clone(), measure.apply([])?)?;
                        }
                        function.requires_draft(selected.requirement(
                            parameter,
                            measure.apply([])?,
                            [],
                        )?)?;
                        Ok(())
                    })
                    .implementation_with(|view| {
                        assert!(matches!(
                            view.declaration().records().concrete_result,
                            Some(Ty::Struct(_))
                        ));
                        Ok(NativeBinding::new(vec![Codec::Value], Codec::Value, |_| {
                            panic!("not executed")
                        }))
                    }),
            )?;
            Ok(())
        }))
        .unwrap();
        let result = set.finish();
        if missing_bound {
            assert!(
                result.is_err(),
                "selected call without bound must reject before factory"
            );
        } else {
            result.unwrap().into_modules().unwrap();
        }
    }
}

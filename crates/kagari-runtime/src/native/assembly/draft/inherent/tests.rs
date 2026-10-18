//! Generic inherent contracts survive both authoring paths without runtime installation.
use crate::native::{
    assembly::{
        ModuleAssembly, ModuleDefinition, ModuleSetBuilder, conflict, draft::TraitImplementation,
    },
    binding::{Codec, NativeBinding, NativeResult},
    builder::ModuleBuilder,
    catalog::DeclarationCatalog,
    declarations::{CallableRequirement, MethodDecl},
    module::NativeModule,
    primitive::NativePrimitive,
    storage::NativeStorage,
    types::{TraitRef, Type},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, PackageId},
};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout},
    ty::Ty,
};
use std::{cell::Cell, rc::Rc};

fn contracts() -> (NativeModule, TraitRef, TraitRef) {
    let mut module = ModuleBuilder::new(
        "inherent_support::contracts",
        &DeclarationCatalog::default(),
    )
    .unwrap();
    let mut hash = module.define_trait("Hash").unwrap();
    hash.define_method(MethodDecl::instance("hash").returns(Type::u64()))
        .unwrap();
    let hash = hash.finish().unwrap();
    let mut view = module.define_trait("View").unwrap();
    view.type_parameter("T").unwrap();
    let view = view.finish().unwrap();
    (module.finish().unwrap(), hash, view)
}

fn identity() -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("generic_inherent".into()),
        path: vec!["vector".into()],
    }
}

fn project_binding() -> NativeBinding {
    NativeBinding::new(
        vec![Codec::Sequence, Codec::Value, Codec::Value],
        Codec::Sequence,
        |_| panic!("registration must not execute a native callback"),
    )
}

#[derive(Clone, Copy)]
struct Proofs {
    group_bound: bool,
    method_bound: bool,
    concrete_impl: bool,
}

impl Proofs {
    const COMPLETE: Self = Self {
        group_bound: true,
        method_bound: true,
        concrete_impl: true,
    };
}

fn assembly(
    proofs: Proofs,
    recipes: Rc<Cell<usize>>,
    prepares: Rc<Cell<usize>>,
) -> NativeResult<ModuleAssembly> {
    let (provider, hash, view) = contracts();
    let mut set = ModuleSetBuilder::new(&provider.catalog())?;
    set.add(ModuleDefinition::new(identity(), move |module| {
        recipes.set(recipes.get() + 1);
        let vector = module.define_type(
            "Vector",
            TypeDefKind::NativeStorage(NativeStorageLayout::Sequence { element: 0 }),
            1,
            |_, _| Ok(()),
        )?;
        module.bind_storage(&vector, NativeStorage::sequence(0))?;
        if proofs.concrete_impl {
            module.trait_implementation_with(1, |parameters| {
                Ok(TraitImplementation {
                    receiver: vector.apply(parameters.to_vec())?,
                    interface: view.apply(parameters.to_vec())?.ty(),
                    bounds: vec![],
                    bindings: vec![],
                })
            })?;
        }
        module.inherent_generic(&vector, |parameters, methods| {
            let item = parameters[0].clone();
            if proofs.group_bound {
                methods.bound(item.clone(), hash.apply([])?.ty())?;
            }
            let concrete = methods.receiver();
            methods.define(
                MethodDecl::instance("project")
                    .parameter("item", item.clone())
                    .returns(view.apply([item.clone()])?.ty())
                    .produces(concrete)
                    .documentation("Return the same vector through its declared view.")
                    .configure(move |function| {
                        let key = function.type_parameter("K")?.ty();
                        function.parameter("key", key.clone())?;
                        if proofs.method_bound {
                            function.bound(key.clone(), hash.apply([])?)?;
                        }
                        function
                            .requires(CallableRequirement::method(item, hash.method("hash")?)?)?;
                        function
                            .requires(CallableRequirement::method(key, hash.method("hash")?)?)?;
                        Ok(())
                    })
                    .implementation_with(move |_| {
                        prepares.set(prepares.get() + 1);
                        Ok(project_binding())
                    }),
            )?;
            Ok(())
        })?;
        module.inherent_generic(&vector, |parameters, methods| {
            methods.define(
                MethodDecl::instance("at")
                    .parameter("index", Type::u64())
                    .returns(parameters[0].clone())
                    .implementation(NativeBinding::primitive(NativePrimitive::VecIndex)),
            )?;
            Ok(())
        })
    }))?;
    set.finish()
}

#[test]
fn generic_receiver_bounds_selected_calls_and_concrete_results_seal_without_preparation() {
    let recipes = Rc::new(Cell::new(0));
    let prepares = Rc::new(Cell::new(0));
    let assembly = assembly(Proofs::COMPLETE, recipes.clone(), prepares.clone()).unwrap();
    let snapshot = assembly.declarations();
    assert_eq!(recipes.get(), 1, "a non-Send recipe runs once");
    assert_eq!(prepares.get(), 0);
    let metadata = &snapshot.modules()[0];
    let module = metadata.records();
    let groups = module
        .implementations
        .iter()
        .enumerate()
        .filter(|(_, group)| group.trait_type.is_none())
        .collect::<Vec<_>>();
    let (project_index, project_group) = groups[0];
    let (index_index, index_group) = groups[1];
    let project_owner = module
        .implementation_id(project_index, metadata.definitions())
        .unwrap();
    let index_owner = module
        .implementation_id(index_index, metadata.definitions())
        .unwrap();
    let method_id = metadata
        .definitions()
        .lookup_child(project_owner, DefinitionKind::Method, "project", 0)
        .unwrap();
    assert_ne!(project_owner, index_owner);
    let project = &project_group.methods[0];
    assert_eq!(project.generic_params[0].owner, project_owner);
    assert_eq!(project.generic_params[1].owner, method_id);
    assert_eq!(project.params[1].ty, project.generic_params[0].as_type());
    assert_eq!(project.params[2].ty, project.generic_params[1].as_type());
    assert_eq!(module.concrete_results[&method_id], project_group.for_type);
    let requirements = &module.callable_requirements[&method_id];
    assert_eq!(
        requirements[0].receiver,
        project.generic_params[0].as_type()
    );
    assert_eq!(
        requirements[1].receiver,
        project.generic_params[1].as_type()
    );
    assert_eq!(
        project_group.bounds[0].ty,
        project.generic_params[0].as_type()
    );
    assert_eq!(project.bounds[0].ty, project.generic_params[1].as_type());
    assert_eq!(
        index_group.methods[0].return_type,
        index_group.generic_params[0].as_type()
    );
    let before = module
        .compiler_input_bytes(metadata.definitions(), &CancellationToken::default())
        .unwrap();
    let prepared = assembly.into_modules().unwrap();
    assert_eq!(recipes.get(), 1);
    assert_eq!(prepares.get(), 1);
    assert_eq!(
        before,
        prepared[0]
            .declaration()
            .compiler_input_bytes(prepared[0].definitions(), &CancellationToken::default())
            .unwrap()
    );
}

#[test]
fn generic_inherent_rejects_missing_group_method_and_result_proofs_at_sealing() {
    for proofs in [
        Proofs {
            group_bound: false,
            ..Proofs::COMPLETE
        },
        Proofs {
            method_bound: false,
            ..Proofs::COMPLETE
        },
        Proofs {
            concrete_impl: false,
            ..Proofs::COMPLETE
        },
    ] {
        let prepares = Rc::new(Cell::new(0));
        assert!(assembly(proofs, Rc::new(Cell::new(0)), prepares.clone()).is_err());
        assert_eq!(
            prepares.get(),
            0,
            "failed proofs must not run adapter factories"
        );
    }
}

#[test]
fn ordinary_builder_rebinds_reused_receiver_parameters_through_the_same_lowerer() {
    let (provider, hash, view) = contracts();
    let mut module = ModuleBuilder::new("generic_inherent::ordinary", &provider.catalog()).unwrap();
    let mut vector = module.define_type("Vector").unwrap();
    let element = vector.type_parameter("T").unwrap();
    vector.sequence_storage(&element).unwrap();
    let vector = vector.finish().unwrap();
    module
        .implement(vector.clone(), |group| {
            let item = group.parameter("T")?.ty();
            // Emitting the trait impl first makes the inherent group's owner differ
            // from the receiver's authoring binder: this exercises real substitution.
            group.trait_impl(view.apply([item.clone()])?, |_| Ok(()))?;
            group.inherent_impl(|methods| {
                let method = methods.define_method(
                    MethodDecl::instance("project")
                        .parameter("item", item.clone())
                        .returns(view.apply([item.clone()])?.ty()),
                )?;
                methods.method(&method, |function| {
                    let key = function.type_parameter("K")?.ty();
                    function.parameter("key", key.clone())?;
                    function.bound(item.clone(), hash.apply([])?)?;
                    function.bound(key.clone(), hash.apply([])?)?;
                    function.requires(CallableRequirement::method(
                        item.clone(),
                        hash.method("hash")?,
                    )?)?;
                    function.requires(CallableRequirement::method(key, hash.method("hash")?)?)?;
                    function.produces(vector.apply([item.clone()])?)
                })?;
                methods.bind(method, project_binding())
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let group = &module.declaration().implementations[1];
    let project = &group.methods[0];
    let Ty::NativeObject(receiver) = &group.for_type else {
        panic!("native receiver")
    };
    assert_eq!(receiver.arguments, [group.generic_params[0].as_type()]);
    assert_eq!(project.params[1].ty, group.generic_params[0].as_type());
    let id = module
        .declaration()
        .implementation_id(1, module.definitions())
        .unwrap();
    let method = module
        .definitions()
        .lookup_child(id, DefinitionKind::Method, "project", 0)
        .unwrap();
    assert_eq!(
        module.declaration().callable_requirements[&method][0].receiver,
        group.generic_params[0].as_type()
    );
    assert_eq!(
        module.declaration().concrete_results[&method],
        group.for_type
    );
    assert!(
        project
            .bounds
            .iter()
            .any(|bound| bound.ty == group.generic_params[0].as_type())
    );
    assert_eq!(project.generic_params[1].owner, method);
}

#[test]
fn failed_groups_are_atomic_and_parameters_cannot_escape_to_another_group() {
    for leak_parameter in [false, true] {
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        set.add(ModuleDefinition::new(identity(), move |module| {
            let vector = module.define_type(
                "Vector",
                TypeDefKind::NativeStorage(NativeStorageLayout::Sequence { element: 0 }),
                1,
                |_, _| Ok(()),
            )?;
            module.bind_storage(&vector, NativeStorage::sequence(0))?;
            assert!(
                module
                    .inherent_generic(&vector, |_, methods| {
                        methods.define(
                            MethodDecl::instance("discarded").implementation(
                                NativeBinding::primitive(NativePrimitive::VecIndex),
                            ),
                        )?;
                        Err::<(), _>(conflict("reject this group"))
                    })
                    .is_err()
            );
            let leaked = module.inherent_generic(&vector, |parameters, methods| {
                methods.define(
                    MethodDecl::instance("at")
                        .parameter("index", Type::u64())
                        .returns(parameters[0].clone())
                        .implementation(NativeBinding::primitive(NativePrimitive::VecIndex)),
                )?;
                Ok(parameters[0].clone())
            })?;
            module.inherent_generic(&vector, |parameters, methods| {
                methods.define(
                    MethodDecl::instance("copy_at")
                        .parameter("index", Type::u64())
                        .returns(if leak_parameter {
                            leaked
                        } else {
                            parameters[0].clone()
                        })
                        .implementation(NativeBinding::primitive(NativePrimitive::VecIndex)),
                )?;
                Ok(())
            })
        }))
        .unwrap();
        let result = set.finish();
        if leak_parameter {
            assert!(
                result.is_err(),
                "a later group must not inherit an unrelated impl binder"
            );
        } else {
            let assembly = result.unwrap();
            assert!(
                assembly.declarations().modules()[0]
                    .records()
                    .implementations
                    .iter()
                    .flat_map(|group| &group.methods)
                    .all(|method| method.name != "discarded")
            );
            assert!(assembly.into_modules().is_ok());
        }
    }
}

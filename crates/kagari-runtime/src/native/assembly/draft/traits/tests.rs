//! Same-batch public authoring proofs, with no prebuilt trait providers.
use crate::native::{
    assembly::{
        ModuleAssembly, ModuleDefinition, ModuleSetBuilder,
        authoring::PendingBinding,
        draft::{ModuleDraft, TraitImplementation, traits::TraitOverride},
        references::{DraftMember, DraftReference},
    },
    binding::{Codec, NativeBinding, NativeResult},
    catalog::DeclarationCatalog,
    declarations::MethodDecl,
    storage::NativeStorage,
    types::Type,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId},
};
use kagari_types::{
    declaration::{AssociatedTypeDef, TypeDefKind, native::NativeStorageLayout},
    ty::{GenericBound, GenericParam, Ty},
};
use rstest::rstest;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

mod execution;

fn owner(name: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("forward".into()),
        path: vec![name.into()],
    }
}

fn path(module: &str, kind: DefinitionKind, name: &str) -> DefinitionPath {
    DefinitionPath {
        module: owner(module),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

struct Contracts {
    hash: DraftReference,
    hash_method: DraftMember,
    view: DraftReference,
    project: DraftReference,
    output: DraftMember,
    project_method: DraftMember,
}

impl Contracts {
    fn reference(module: &mut ModuleDraft<'_>) -> NativeResult<Self> {
        let hash = module.reference_trait(path("contracts", DefinitionKind::Trait, "Hash"), 0)?;
        let hash_method = module.reference_method(&hash, "hash", 1)?;
        let view = module.reference_trait(path("contracts", DefinitionKind::Trait, "View"), 1)?;
        let project =
            module.reference_trait(path("contracts", DefinitionKind::Trait, "Project"), 1)?;
        let output = module.reference_associated_type(&project, "Output", 0)?;
        let project_method = module.reference_method(&project, "project", 1)?;
        Ok(Self {
            hash,
            hash_method,
            view,
            project,
            output,
            project_method,
        })
    }
}

fn bound(ty: &Type, interface: Type) -> GenericBound {
    let Ty::Trait(interface) = interface.abi() else {
        panic!("trait")
    };
    GenericBound {
        ty: ty.abi().clone(),
        constraints: vec![interface.clone()],
    }
}

#[derive(Clone, Copy, Debug)]
enum Fault {
    None,
    ImplBound,
    MethodBound,
    ConcreteResult,
    InherentBound,
}

fn contracts(fault: Fault) -> ModuleDefinition {
    ModuleDefinition::new(owner("contracts"), move |module| {
        let refs = Contracts::reference(module)?;
        let mut hash = module.trait_method(
            refs.hash.id(),
            MethodDecl::instance("hash")
                .parameter("salt", refs.hash_method.parameter(0)?)
                .returns(Type::u64()),
        )?;
        hash.generic_params.push(GenericParam {
            owner: refs.hash_method.id(),
            position: 0,
        });
        module.define_trait("Hash", 0, |record, _| {
            record.methods.push(hash);
            Ok(())
        })?;
        module.define_trait("View", 1, |_, _| Ok(()))?;
        let item = refs.project.parameter(0)?;
        let output =
            refs.output
                .projection(refs.project.receiver()?, refs.project.apply([item])?, [])?;
        let key = refs.project_method.parameter(0)?;
        let mut project = module.trait_method(
            refs.project.id(),
            MethodDecl::instance("project")
                .parameter("key", key.clone())
                .returns(refs.view.apply([output])?),
        )?;
        project.generic_params.push(GenericParam {
            owner: refs.project_method.id(),
            position: 0,
        });
        if !matches!(fault, Fault::MethodBound) {
            project.bounds.push(bound(&key, refs.hash.apply([])?));
        }
        module.define_trait("Project", 1, |record, _| {
            record.methods.push(project);
            record.associated_types.push(AssociatedTypeDef {
                declaration: refs.output.id(),
                generic_params: vec![],
                parameter_bounds: vec![],
                bounds: vec![],
            });
            Ok(())
        })?;
        Ok(())
    })
}

fn project_binding() -> NativeBinding {
    NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, |_| {
        panic!("declaration/preparation must not execute a body")
    })
}

fn assembly(
    reverse: bool,
    fault: Fault,
    factories: Rc<Cell<usize>>,
) -> NativeResult<ModuleAssembly> {
    let implementation = ModuleDefinition::new(owner("implementation"), move |module| {
        let refs = Contracts::reference(module)?;
        let vector = module.define_type(
            "Vector",
            TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
            1,
            |_, _| Ok(()),
        )?;
        module.bind_storage(&vector, NativeStorage::data::<u32>())?;
        module.trait_implementation_with(1, |arguments| {
            Ok(TraitImplementation {
                receiver: vector.apply(arguments.to_vec())?,
                interface: refs.view.apply(arguments.to_vec())?,
                bounds: vec![],
                bindings: vec![],
            })
        })?;
        module.trait_implementation_with(1, |arguments| {
            let item = refs.project.parameter(0)?;
            let key = refs.project_method.parameter(0)?;
            let output = refs.output.projection(
                refs.project.receiver()?,
                refs.project.apply([item.clone()])?,
                [],
            )?;
            let requirements = [item, key.clone(), output]
                .into_iter()
                .map(|receiver| {
                    refs.hash_method
                        .requirement(receiver, refs.hash.apply([])?, [key.clone()])
                })
                .collect::<NativeResult<Vec<_>>>()?;
            let pending = PendingBinding::implementation_with(move |preparation| {
                factories.set(factories.get() + 1);
                let declaration = preparation.declaration().records();
                assert_eq!(declaration.callable_requirements.len(), 3);
                assert_eq!(
                    declaration.concrete_result.as_ref(),
                    Some(&declaration.function.params[0].ty)
                );
                Ok(project_binding())
            });
            let mut entry = TraitOverride::new(refs.project_method.clone(), pending).produces(
                if matches!(fault, Fault::ConcreteResult) {
                    Type::bool()
                } else {
                    refs.project.receiver()?
                },
            );
            for requirement in requirements {
                entry = entry.requires(requirement);
            }
            Ok(TraitImplementation {
                receiver: vector.apply(arguments.to_vec())?,
                interface: refs.output.bind(
                    refs.project.apply(arguments.to_vec())?,
                    arguments[0].clone(),
                )?,
                bounds: if matches!(fault, Fault::ImplBound) {
                    vec![]
                } else {
                    vec![bound(&arguments[0], refs.hash.apply([])?)]
                },
                bindings: vec![],
            }
            .with_overrides([entry]))
        })?;
        module.inherent_generic(&vector, |parameters, methods| {
            if !matches!(fault, Fault::InherentBound) {
                methods.bound(parameters[0].clone(), refs.hash.apply([])?)?;
            }
            let item = parameters[0].clone();
            methods.define(
                MethodDecl::instance("inspect")
                    .returns(Type::u64())
                    .configure(move |function| {
                        let key = function.type_parameter("K")?.ty();
                        function.parameter("key", key.clone())?;
                        function.bound_draft(key.clone(), refs.hash.apply([])?)?;
                        function.requires_draft(refs.hash_method.requirement(
                            item,
                            refs.hash.apply([])?,
                            [key.clone()],
                        )?)?;
                        function.requires_draft(refs.hash_method.requirement(
                            key.clone(),
                            refs.hash.apply([])?,
                            [key],
                        )?)?;
                        Ok(())
                    })
                    .implementation(NativeBinding::new(
                        vec![Codec::Value, Codec::Value],
                        Codec::Exact(Type::u64()),
                        |_| panic!("not executed"),
                    )),
            )?;
            Ok(())
        })
    });
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default())?;
    if reverse {
        set.add(implementation)?;
        set.add(contracts(fault))?;
    } else {
        set.add(contracts(fault))?;
        set.add(implementation)?;
    }
    set.finish()
}

#[test]
fn same_batch_generic_override_facts_and_inherent_requirements_are_order_independent() {
    let mut canonical = None;
    for reverse in [false, true] {
        let factories = Rc::new(Cell::new(0));
        let assembly = assembly(reverse, Fault::None, factories.clone()).unwrap();
        assert_eq!(factories.get(), 0);
        let snapshot = assembly.declarations();
        let metadata = snapshot
            .modules()
            .iter()
            .find(|module| module.records().identity == owner("implementation"))
            .unwrap();
        let module = metadata.records();
        let implementation = &module.implementations[1];
        let method = &implementation.methods[0];
        let id = method.generic_params[1].owner;
        assert_eq!(
            method.generic_params[0].owner,
            implementation.generic_params[0].owner
        );
        let item = method.generic_params[0].as_type();
        let key = method.generic_params[1].as_type();
        let requirements = &module.callable_requirements[&id];
        assert_eq!(
            requirements
                .iter()
                .map(|r| r.receiver.clone())
                .collect::<Vec<_>>(),
            [item.clone(), key.clone(), item]
        );
        assert!(requirements.iter().all(|r| r.arguments == [key.clone()]));
        assert_eq!(module.concrete_results[&id], implementation.for_type);
        let bytes = module
            .compiler_input_bytes(metadata.definitions(), &CancellationToken::default())
            .unwrap();
        if let Some(previous) = &canonical {
            assert_eq!(&bytes, previous);
        }
        canonical = Some(bytes.clone());
        let prepared = assembly.into_modules().unwrap();
        assert_eq!(factories.get(), 1);
        let prepared = prepared
            .iter()
            .find(|module| module.declaration().identity == owner("implementation"))
            .unwrap();
        assert_eq!(
            bytes,
            prepared
                .declaration()
                .compiler_input_bytes(prepared.definitions(), &CancellationToken::default())
                .unwrap()
        );
    }
}

#[rstest]
#[case::impl_bound(Fault::ImplBound)]
#[case::method_bound(Fault::MethodBound)]
#[case::concrete_result(Fault::ConcreteResult)]
#[case::inherent_bound(Fault::InherentBound)]
fn override_and_inherent_facts_keep_all_existing_proof_obligations(#[case] fault: Fault) {
    let factories = Rc::new(Cell::new(0));
    let error = assembly(true, fault, factories.clone())
        .err()
        .expect("missing proof must reject");
    let expected = if matches!(fault, Fault::ConcreteResult) {
        "native concrete result does not implement its return interface"
    } else {
        "selected calls differ from their declared contracts"
    };
    assert!(error.to_string().contains(expected), "{fault:?}: {error}");
    assert_eq!(factories.get(), 0);
}

#[rstest]
#[case::missing_member("absent", false, 0)]
#[case::wrong_category("hash", true, 1)]
#[case::wrong_arity("hash", false, 0)]
fn unused_forward_members_still_check_existence_category_and_arity(
    #[case] name: &'static str,
    #[case] associated: bool,
    #[case] arity: usize,
) {
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner("references"), move |module| {
        let hash = module.reference_trait(path("contracts", DefinitionKind::Trait, "Hash"), 0)?;
        if associated {
            module.reference_associated_type(&hash, name, arity)?;
        } else {
            module.reference_method(&hash, name, arity)?;
        }
        Ok(())
    }))
    .unwrap();
    set.add(contracts(Fault::None)).unwrap();
    assert!(set.finish().is_err());
}

#[test]
fn member_inputs_reject_foreign_scope_wrong_owner_category_and_duplicate_outputs() {
    let foreign = Rc::new(RefCell::new(None));
    let captured = foreign.clone();
    let mut first = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    first.add(contracts(Fault::None)).unwrap();
    first
        .add(ModuleDefinition::new(owner("capture"), move |module| {
            *captured.borrow_mut() = Some(Contracts::reference(module)?);
            Ok(())
        }))
        .unwrap();
    first.finish().unwrap();
    let mut second = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    second.add(contracts(Fault::None)).unwrap();
    second
        .add(ModuleDefinition::new(owner("checks"), move |module| {
            let refs = Contracts::reference(module)?;
            let foreign = foreign.borrow();
            let foreign = foreign.as_ref().unwrap();
            assert!(module.reference_method(&foreign.hash, "hash", 1).is_err());
            let nominal = module.define_type("Nominal", TypeDefKind::Struct, 0, |_, _| Ok(()))?;
            assert!(module.reference_method(&nominal, "hash", 1).is_err());
            assert!(
                module
                    .member(refs.hash.id(), DefinitionKind::Method, "foreign_write")
                    .is_err()
            );
            assert!(
                refs.hash_method
                    .requirement(Type::i32(), refs.view.apply([Type::i32()])?, [Type::i32()])
                    .is_err()
            );
            assert!(
                refs.output
                    .requirement(Type::i32(), refs.project.apply([Type::i32()])?, [])
                    .is_err()
            );
            assert!(
                refs.hash_method
                    .requirement(Type::i32(), refs.hash.apply([])?, [])
                    .is_err()
            );
            let bound = refs
                .output
                .bind(refs.project.apply([Type::i32()])?, Type::i32())?;
            assert!(refs.output.bind(bound, Type::i32()).is_err());
            assert!(
                module
                    .trait_implementation_with(0, |_| Ok(TraitImplementation {
                        receiver: Type::i32(),
                        interface: refs.hash.apply([])?,
                        bounds: vec![],
                        bindings: vec![],
                    }
                    .with_overrides([TraitOverride::new(
                        foreign.hash_method.clone(),
                        PendingBinding::implementation(project_binding())
                    )])))
                    .is_err()
            );
            let requirement = foreign.hash_method.requirement(
                Type::i32(),
                foreign.hash.apply([])?,
                [Type::i32()],
            )?;
            module.inherent(nominal.apply([])?, |methods| {
                methods.define(
                    MethodDecl::instance("probe")
                        .configure(move |function| {
                            assert!(function.requires_draft(requirement).is_err());
                            Ok(())
                        })
                        .implementation(NativeBinding::new(
                            vec![Codec::Value],
                            Codec::Exact(Type::unit()),
                            |_| panic!("not executed"),
                        )),
                )?;
                Ok(())
            })
        }))
        .unwrap();
    second.finish().unwrap();
}

#[test]
fn overrides_reject_wrong_owner_and_duplicate_raw_and_fact_entries() {
    for wrong_owner in [false, true] {
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        set.add(contracts(Fault::None)).unwrap();
        set.add(ModuleDefinition::new(
            owner("bad_override"),
            move |module| {
                let refs = Contracts::reference(module)?;
                let binding = || PendingBinding::implementation(project_binding());
                let entry = TraitOverride::new(
                    if wrong_owner {
                        refs.hash_method
                    } else {
                        refs.project_method
                    },
                    binding(),
                );
                let result = module.trait_implementation_with(0, |_| {
                    Ok(TraitImplementation {
                        receiver: Type::i32(),
                        interface: refs
                            .output
                            .bind(refs.project.apply([Type::i32()])?, Type::i32())?,
                        bounds: vec![],
                        bindings: if wrong_owner {
                            vec![]
                        } else {
                            vec![("project".into(), binding())]
                        },
                    }
                    .with_overrides([entry]))
                });
                assert!(result.is_err());
                Ok(())
            },
        ))
        .unwrap();
        set.finish().unwrap();
    }
}

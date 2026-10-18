//! Assembly-level default proofs: complete authoring, sealing and real preparation.
mod selected_calls;
use crate::native::{
    adapter::NativeAdapter,
    assembly::{
        ModuleAssembly, ModuleDefinition, ModuleSetBuilder, conflict,
        draft::{ModuleDraft, TraitImplementation, defaults::DefaultDefinition},
    },
    binding::{Codec, NativeBinding, NativeResult},
    catalog::DeclarationCatalog,
    declarations::MethodDecl,
    types::Type,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
        table::DefinitionId,
    },
};
use kagari_stdlib::{catalog as standard_catalog, namespaces};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        AssociatedTypeDef, FnDecl, TypeDefKind, VariantDef, native::NativeStorageLayout,
        requirement::NativeCallableRequirement,
    },
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Default)]
struct Counts {
    factories: Arc<AtomicUsize>,
    prepares: Arc<AtomicUsize>,
}

impl Counts {
    fn assert(&self, factories: usize, prepares: usize) {
        assert_eq!(self.factories.load(Ordering::SeqCst), factories);
        assert_eq!(self.prepares.load(Ordering::SeqCst), prepares);
    }
}

struct Adapter {
    counts: Counts,
    result: Codec,
    method_generics: usize,
}

impl NativeAdapter<(), ()> for Adapter {
    fn prepare(self, declaration: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
        self.counts.prepares.fetch_add(1, Ordering::SeqCst);
        assert_eq!(declaration.params[0].name, "self");
        assert!(matches!(declaration.params[0].ty, Ty::SelfType(_)));
        assert_eq!(declaration.generic_params.len(), self.method_generics);
        assert!(!declaration.name.starts_with("__default_"));
        Ok(NativeBinding::new(
            vec![Codec::Value; declaration.params.len()],
            self.result,
            |_| panic!("preparing a default must not execute its body"),
        ))
    }
}

fn owner(name: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("default-proof".into()),
        path: vec![name.into()],
    }
}

fn path(package: &str, module: &str, kind: DefinitionKind, name: &str) -> DefinitionPath {
    DefinitionPath {
        module: namespaces::module(package, module),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

fn nominal(declaration: DefinitionId, arguments: Vec<Ty>) -> NominalTy {
    NominalTy {
        declaration,
        arguments,
        associated_types: BTreeMap::new(),
    }
}

fn parameter(owner: DefinitionId, position: usize) -> Ty {
    GenericParam { owner, position }.as_type()
}

fn ty(module: &ModuleDraft<'_>, value: Ty) -> NativeResult<Type> {
    Type::from_semantic(value, &module.definitions())
}

fn projection(receiver: Ty, interface: NominalTy, member: DefinitionId) -> Ty {
    Ty::Projection {
        receiver: Box::new(receiver),
        interface: Box::new(interface),
        member,
        arguments: vec![],
    }
}

fn bound(ty: Ty, interface: NominalTy) -> GenericBound {
    GenericBound {
        ty,
        constraints: vec![interface],
    }
}

struct Iteration {
    iterable: DefinitionId,
    item: DefinitionId,
    cursor: DefinitionId,
    iter: DefinitionId,
    iterator: DefinitionId,
    iterator_item: DefinitionId,
    next: DefinitionId,
}

impl Iteration {
    fn standard(module: &mut ModuleDraft<'_>) -> NativeResult<Self> {
        let iterable = module
            .reference_trait(path("core", "iter", DefinitionKind::Trait, "Iterable"), 0)?
            .id();
        Self::with_iterable(module, iterable)
    }

    fn with_iterable(module: &mut ModuleDraft<'_>, iterable: DefinitionId) -> NativeResult<Self> {
        let iterator = module
            .reference_trait(path("core", "iter", DefinitionKind::Trait, "Iterator"), 0)?
            .id();
        let definitions = module.definitions();
        let child = |owner, kind, name| {
            definitions
                .lookup_child(owner, kind, name, 0)
                .ok_or_else(|| conflict("missing iteration member"))
        };
        Ok(Self {
            iterable,
            item: child(iterable, DefinitionKind::AssociatedType, "Item")?,
            cursor: child(iterable, DefinitionKind::AssociatedType, "Iter")?,
            iter: child(iterable, DefinitionKind::Method, "iter")?,
            iterator,
            iterator_item: child(iterator, DefinitionKind::AssociatedType, "Item")?,
            next: child(iterator, DefinitionKind::Method, "next")?,
        })
    }

    fn iterable(&self, item: Ty, cursor: Option<Ty>) -> NominalTy {
        let mut interface = nominal(self.iterable, vec![]);
        interface.associated_types.insert(self.item, item);
        if let Some(cursor) = cursor {
            interface.associated_types.insert(self.cursor, cursor);
        }
        interface
    }

    fn iterator(&self, item: Ty) -> NominalTy {
        let mut interface = nominal(self.iterator, vec![]);
        interface.associated_types.insert(self.iterator_item, item);
        interface
    }

    fn source(
        &self,
        receiver: Ty,
        item: Ty,
        cursor: Ty,
    ) -> (Vec<GenericBound>, Vec<NativeCallableRequirement>) {
        let iterable = self.iterable(item.clone(), Some(cursor.clone()));
        let iterator = self.iterator(item);
        (
            vec![
                bound(receiver.clone(), iterable.clone()),
                bound(cursor.clone(), iterator.clone()),
            ],
            vec![
                NativeCallableRequirement {
                    receiver,
                    interface: iterable,
                    member: self.iter,
                    arguments: vec![],
                },
                NativeCallableRequirement {
                    receiver: cursor,
                    interface: iterator,
                    member: self.next,
                    arguments: vec![],
                },
            ],
        )
    }
}

fn list_assembly(counts: Counts, implement: bool, bad_codec: bool) -> NativeResult<ModuleAssembly> {
    let providers = DeclarationCatalog::from_declarations(
        &standard_catalog::declarations().map_err(conflict)?,
    )?;
    let mut set = ModuleSetBuilder::new(&providers)?;
    set.add(ModuleDefinition::new(owner("list"), move |module| {
        let iteration = Iteration::standard(module)?;
        let list_id = module.definition(DefinitionKind::Trait, "List")?;
        let item = parameter(list_id, 0);
        let iterable = iteration.iterable(item.clone(), None);
        let list = module.define_trait("List", 1, |record, _| {
            record.supertraits.push(iterable);
            Ok(())
        })?;
        let vec_type = module.reference_type(
            path("alloc", "vec", DefinitionKind::AssociatedType, "Vec"),
            TypeDefKind::NativeStorage(NativeStorageLayout::Sequence { element: 0 }),
            1,
        )?;
        if implement {
            module.trait_implementation_with(1, |arguments| {
                Ok(TraitImplementation {
                    receiver: vec_type.apply(arguments.to_vec())?,
                    interface: list.apply(arguments.to_vec())?,
                    bounds: vec![],
                    bindings: vec![],
                })
            })?;
        }
        let receiver = Ty::SelfType(list_id);
        let cursor = projection(
            receiver.clone(),
            iteration.iterable(item.clone(), None),
            iteration.cursor,
        );
        let (assumptions, requirements) =
            iteration.source(receiver.clone(), item.clone(), cursor.clone());
        let inputs = [receiver, item.clone(), cursor]
            .into_iter()
            .map(|value| ty(module, value))
            .collect::<NativeResult<Vec<_>>>()?;
        let item_type = ty(module, item)?;
        let definition = MethodDecl::instance("reversed")
            .returns(list.apply([item_type.clone()])?)
            .produces(vec_type.apply([item_type])?)
            .documentation("Copy into a concrete Vec while exposing a List.")
            .implementation_with(move |preparation| {
                counts.factories.fetch_add(1, Ordering::SeqCst);
                let target = preparation.declaration();
                assert_eq!(target.records().callable_requirements.len(), 2);
                assert!(matches!(
                    target.records().concrete_result,
                    Some(Ty::NativeObject(_))
                ));
                assert_eq!(
                    preparation
                        .catalog()
                        .declarations
                        .get_id(target.records().declaration),
                    Some(target.records())
                );
                Ok(Adapter {
                    counts,
                    result: if bad_codec {
                        Codec::Exact(Type::bool())
                    } else {
                        Codec::Sequence
                    },
                    method_generics: 0,
                })
            });
        module.define_trait_default(
            &list,
            DefaultDefinition::new(definition)
                .arguments(inputs)
                .assumptions(assumptions)
                .requirements(requirements),
        )?;
        Ok(())
    }))?;
    set.finish()
}

#[test]
fn list_default_seals_hidden_cursor_and_concrete_vec_without_preparation() {
    let counts = Counts::default();
    let assembly = list_assembly(counts.clone(), true, false).unwrap();
    counts.assert(0, 0);
    let snapshot = assembly.declarations();
    let record = snapshot.modules()[0].records();
    let definitions = snapshot.modules()[0].definitions();
    let contract = &record.traits[0];
    let CallableImplementation::NativeDefault(application) = &contract.methods[0].implementation
    else {
        panic!("missing default application")
    };
    let template = &record.functions[0];
    let p = |position| parameter(application.declaration, position);
    assert_eq!(template.generic_params.len(), 3);
    assert!(matches!(application.arguments[2], Ty::Projection { .. }));
    assert_eq!(
        record.callable_requirements[&application.declaration][0].receiver,
        p(0)
    );
    assert_eq!(
        record.callable_requirements[&application.declaration][1].receiver,
        p(2)
    );
    let Ty::NativeObject(vec) = &record.concrete_results[&application.declaration] else {
        panic!("missing concrete Vec")
    };
    assert_eq!(
        definitions
            .resolve(vec.declaration)
            .unwrap()
            .last()
            .unwrap()
            .name,
        "Vec"
    );
    assert_eq!(vec.arguments, vec![p(1)]);
    let Ty::Trait(result) = &template.return_type else {
        panic!("missing List view")
    };
    assert_eq!(
        definitions
            .resolve(result.declaration)
            .unwrap()
            .last()
            .unwrap()
            .name,
        "List"
    );
    assert_eq!(result.arguments, vec![p(1)]);
    let method_id = definitions
        .lookup_child(result.declaration, DefinitionKind::Method, "reversed", 0)
        .unwrap();
    assert_ne!(method_id, application.declaration);
    assert!(
        assembly.drafts[0]
            .bindings
            .contains_key(&application.declaration)
    );
    assert!(!assembly.drafts[0].bindings.contains_key(&method_id));
    assert_eq!(
        record.documentation[&method_id],
        "Copy into a concrete Vec while exposing a List."
    );
    assert!(
        record.implementations[0].methods.is_empty(),
        "impl must inherit the lowered default"
    );
    let before = record
        .compiler_input_bytes(definitions, &CancellationToken::default())
        .unwrap();
    let modules = assembly.into_modules().unwrap();
    counts.assert(1, 1);
    assert_eq!(
        before,
        modules[0]
            .declaration()
            .compiler_input_bytes(modules[0].definitions(), &CancellationToken::default())
            .unwrap()
    );
}

#[test]
fn default_concrete_result_proof_precedes_factory_and_codec_checks_follow_it() {
    let counts = Counts::default();
    assert!(list_assembly(counts.clone(), false, false).is_err());
    counts.assert(0, 0);
    let assembly = list_assembly(counts.clone(), true, true).unwrap();
    counts.assert(0, 0);
    let snapshot = assembly.declarations();
    let error = assembly
        .into_modules()
        .expect_err("incompatible concrete-result codec");
    assert!(error.message().contains("codec"), "{error:?}");
    counts.assert(1, 1);
    assert_eq!(
        snapshot.modules().len(),
        1,
        "failed preparation must not invalidate declarations"
    );
}

fn paired_assembly(counts: Counts) -> NativeResult<ModuleAssembly> {
    let providers = DeclarationCatalog::from_declarations(
        &standard_catalog::declarations().map_err(conflict)?,
    )?;
    let mut set = ModuleSetBuilder::new(&providers)?;
    set.add(ModuleDefinition::new(owner("iterable"), move |module| {
        let owner = module.definition(DefinitionKind::Trait, "Iterable")?;
        let item_member = module.member(owner, DefinitionKind::AssociatedType, "Item")?;
        let cursor_member = module.member(owner, DefinitionKind::AssociatedType, "Iter")?;
        module.member(owner, DefinitionKind::Method, "iter")?;
        let iteration = Iteration::with_iterable(module, owner)?;
        let receiver = Ty::SelfType(owner);
        let item = projection(receiver.clone(), nominal(owner, vec![]), item_member);
        let cursor = projection(receiver.clone(), nominal(owner, vec![]), cursor_member);
        let iter = module.trait_method(
            owner,
            MethodDecl::instance("iter").returns(ty(module, cursor.clone())?),
        )?;
        let interface = module.define_trait("Iterable", 0, |record, _| {
            record.associated_types.extend([
                AssociatedTypeDef {
                    declaration: item_member,
                    generic_params: vec![],
                    parameter_bounds: vec![],
                    bounds: vec![],
                },
                AssociatedTypeDef {
                    declaration: cursor_member,
                    generic_params: vec![],
                    parameter_bounds: vec![],
                    bounds: vec![iteration.iterator(item.clone())],
                },
            ]);
            record.methods.push(iter);
            Ok(())
        })?;
        for name in ["zip", "chain"] {
            let method_id = module.member(owner, DefinitionKind::Method, name)?;
            let right = parameter(method_id, 0);
            let right_item = if name == "zip" {
                projection(right.clone(), nominal(owner, vec![]), item_member)
            } else {
                item.clone()
            };
            let right_interface = if name == "zip" {
                nominal(owner, vec![])
            } else {
                iteration.iterable(item.clone(), None)
            };
            let right_cursor = projection(right.clone(), right_interface.clone(), cursor_member);
            let mut arguments = vec![receiver.clone(), item.clone(), right.clone()];
            if name == "zip" {
                arguments.push(right_item.clone());
            }
            arguments.extend([cursor.clone(), right_cursor.clone()]);
            let arguments = arguments
                .into_iter()
                .map(|value| ty(module, value))
                .collect::<NativeResult<Vec<_>>>()?;
            let (mut assumptions, mut requirements) =
                iteration.source(receiver.clone(), item.clone(), cursor.clone());
            let (right_bounds, right_calls) =
                iteration.source(right.clone(), right_item.clone(), right_cursor);
            assumptions.extend(right_bounds);
            requirements.extend(right_calls);
            let factory_counts = counts.clone();
            let definition = MethodDecl::instance(name)
                .returns(ty(module, Ty::Tuple(vec![item.clone(), right_item]))?)
                .configure(|function| {
                    let right = function.type_parameter("R")?.ty();
                    function.parameter("other", right)
                })
                .implementation_with(move |_| {
                    factory_counts.factories.fetch_add(1, Ordering::SeqCst);
                    Ok(Adapter {
                        counts: factory_counts,
                        result: Codec::Value,
                        method_generics: 1,
                    })
                });
            module.define_trait_default(
                &interface,
                DefaultDefinition::new(definition)
                    .arguments(arguments)
                    .bounds([bound(right, right_interface)])
                    .assumptions(assumptions)
                    .requirements(requirements),
            )?;
        }
        Ok(())
    }))?;
    set.finish()
}

#[test]
fn sealed_paired_defaults_preserve_distinct_source_mappings_and_prepare() {
    let counts = Counts::default();
    let assembly = paired_assembly(counts.clone()).unwrap();
    counts.assert(0, 0);
    let snapshot = assembly.declarations();
    let module = snapshot.modules()[0].records();
    for (name, right_item, left_cursor, right_cursor) in [("zip", 3, 4, 5), ("chain", 1, 3, 4)] {
        let method = module.traits[0]
            .methods
            .iter()
            .find(|method| method.name == name)
            .unwrap();
        let CallableImplementation::NativeDefault(application) = &method.implementation else {
            panic!("not lowered")
        };
        let p = |position| parameter(application.declaration, position);
        let calls = &module.callable_requirements[&application.declaration];
        assert_eq!(calls.len(), 4);
        assert_eq!(
            calls
                .iter()
                .map(|call| call.receiver.clone())
                .collect::<Vec<_>>(),
            vec![p(0), p(left_cursor), p(2), p(right_cursor)]
        );
        let template = module
            .functions
            .iter()
            .find(|function| {
                function.implementation == CallableImplementation::Native(application.declaration)
            })
            .unwrap();
        assert_eq!(template.generic_params.len(), right_cursor + 1);
        assert_eq!(template.return_type, Ty::Tuple(vec![p(1), p(right_item)]));
        assert_eq!(
            calls[1]
                .interface
                .associated_types
                .values()
                .collect::<Vec<_>>(),
            vec![&p(1)]
        );
        assert_eq!(
            calls[3]
                .interface
                .associated_types
                .values()
                .collect::<Vec<_>>(),
            vec![&p(right_item)]
        );
        assert_eq!(calls[0].member, calls[2].member);
        assert_eq!(calls[1].member, calls[3].member);
    }
    let bytes = module
        .compiler_input_bytes(snapshot.modules()[0].definitions(), &Default::default())
        .unwrap();
    let modules = assembly.into_modules().unwrap();
    counts.assert(2, 2);
    assert_eq!(
        bytes,
        modules[0]
            .declaration()
            .compiler_input_bytes(modules[0].definitions(), &Default::default())
            .unwrap()
    );
}

#[test]
fn ordinary_default_submission_is_atomic_and_impls_expand_after_defaults() {
    let counts = Counts::default();
    let factory_counts = counts.clone();
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner("data"), |module| {
        let interface = module.reference_trait(
            path("default-proof", "ordinary", DefinitionKind::Trait, "Run"),
            0,
        )?;
        let data = module.define_type("Data", TypeDefKind::Enum, 0, |record, _| {
            record.variants.push(VariantDef {
                name: "Value".into(),
                payload: vec![],
                reports_failure: false,
            });
            Ok(())
        })?;
        module.trait_implementation(data.apply([])?, interface.apply([])?, vec![])
    }))
    .unwrap();
    set.add(ModuleDefinition::new(owner("ordinary"), move |module| {
        let interface = module.define_trait("Run", 0, |_, _| Ok(()))?;
        assert!(
            module
                .define_trait_default(
                    &interface,
                    MethodDecl::instance("run")
                        .configure(|_| Err(conflict("reject configure")))
                        .implementation_with(|_| -> NativeResult<NativeBinding> {
                            panic!("factory before seal")
                        })
                )
                .is_err()
        );
        module.define_trait_default(
            &interface,
            MethodDecl::instance("run")
                .returns(Type::u64())
                .implementation_with(move |_| {
                    factory_counts.factories.fetch_add(1, Ordering::SeqCst);
                    Ok(Adapter {
                        counts: factory_counts,
                        result: Codec::Exact(Type::u64()),
                        method_generics: 0,
                    })
                }),
        )?;
        assert!(
            module
                .define_trait_default(
                    &interface,
                    MethodDecl::instance("run").implementation_with(
                        |_| -> NativeResult<NativeBinding> { panic!("duplicate factory") }
                    )
                )
                .is_err()
        );
        Ok(())
    }))
    .unwrap();
    let assembly = set.finish().unwrap();
    counts.assert(0, 0);
    let snapshot = assembly.declarations();
    assert!(
        snapshot.modules()[0].records().implementations[0]
            .methods
            .is_empty()
    );
    assert!(
        snapshot.modules()[0]
            .records()
            .dependencies
            .contains(&owner("ordinary"))
    );
    assert_eq!(snapshot.modules()[1].records().functions.len(), 1);
    assert!(matches!(
        snapshot.modules()[1].records().traits[0].methods[0].implementation,
        CallableImplementation::NativeDefault(_)
    ));
    assembly.into_modules().unwrap();
    counts.assert(1, 1);
}

use super::DeclarationCatalog;
use crate::{
    Runtime,
    interpreter_tests::common::{compile_test_bytecode, round_trip_bytecode},
    module::VerifiedProgram,
    native::{
        assembly::{ModuleDefinition, ModuleSetBuilder},
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        module::NativeModule,
        types::Type,
    },
    test_support::native::program::{ProgramFixture, free_function_program},
};
use kagari_bytecode::{
    module::BytecodeModule,
    program::{ModuleRef, verified::VerifiedBytecodeProgram},
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, map::DefinitionContext,
    metadata::DefinitionMetadata,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::types::{PublicItem, TraitContract};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        TypeDeclaration, TypeDef, TypeDefKind,
        host::{HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess},
        module::ModuleDecl,
        names::{ExportName, NameNamespace},
    },
    language::{Protocol, binding},
    range::RangeKind,
    ty::Ty,
};
use std::{slice, sync::Arc};

fn compiled(text: &str) -> VerifiedBytecodeProgram {
    let decoded = round_trip_bytecode(compile_test_bytecode(text));
    VerifiedBytecodeProgram::from_metadata(decoded, &Default::default()).unwrap()
}

fn path(owner: &ModuleIdentity, kind: DefinitionKind, name: &str) -> DefinitionPath {
    DefinitionPath {
        module: owner.clone(),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

#[test]
fn verified_public_owners_support_pre_engine_authoring_and_importer_scoped_admission() {
    let program = compiled(include_str!(
        "../../../tests/fixtures/native/authored-contracts.kgr"
    ));
    let owner = &program.program().modules[program.program().root.index()].identity;
    let catalog = DeclarationCatalog::default()
        .with_verified_modules(&program, [owner])
        .unwrap();
    let signal = path(owner, DefinitionKind::Enum, "Signal");
    let read = path(owner, DefinitionKind::Trait, "Read");
    let ty = catalog.type_reference(&signal).unwrap();
    assert!(ty.apply([]).is_err());
    let applied = ty.apply([Type::i32()]).unwrap();
    let variant = ty.variant("Value").unwrap();
    let mut value = signal.clone();
    value.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Variant,
        name: "Value".into(),
        occurrence: 0,
    });
    assert_eq!(
        variant
            .definitions()
            .resolve(variant.id())
            .unwrap()
            .to_path(),
        value
    );
    let interface = catalog.trait_reference(&read).unwrap();
    let method = interface.method("read").unwrap();
    assert_eq!(
        interface.definitions().parent(method.id()).unwrap(),
        Some(interface.id())
    );
    interface
        .apply([])
        .unwrap()
        .associated("Item", applied)
        .unwrap();
    assert!(
        catalog
            .type_reference(&path(owner, DefinitionKind::Enum, "Hidden"))
            .is_err()
    );

    let mut set = ModuleSetBuilder::new(&catalog).unwrap();
    assert!(
        set.add(ModuleDefinition::new(owner.clone(), |_| Ok(())))
            .unwrap_err()
            .to_string()
            .contains("duplicate or external module owner")
    );
    let facade = ModuleIdentity::single_file("facade");
    let envelope = path(owner, DefinitionKind::Struct, "Envelope");
    let exported_trait = read.clone();
    let unused = envelope.clone();
    set.add(ModuleDefinition::new(facade.clone(), move |module| {
        let record = module.reference_type(envelope.clone(), TypeDefKind::Struct, 1)?;
        let applied = record.apply([Type::i32()])?;
        assert!(matches!(applied.abi(), Ty::Struct(ty)
            if applied.definitions().unwrap().lookup(&envelope) == Some(ty.declaration)
                && ty.arguments.as_slice() == slice::from_ref(Type::i32().abi())));
        module.export_path(ExportName::new(NameNamespace::Type, "Read"), exported_trait)?;
        module.export_path(ExportName::new(NameNamespace::Value, "Value"), value)
    }))
    .unwrap();
    let assembly = set.finish().unwrap();
    let snapshot = assembly.declarations();
    assert_eq!(snapshot.modules().len(), 1);
    assert_eq!(snapshot.modules()[0].records().identity, facade);
    assert_eq!(
        snapshot.catalog().authored_origin(owner),
        Some(program.product_identity())
    );
    let modules = assembly.into_modules().unwrap();
    let requirements = modules[0].catalog().authored_contracts();
    assert!(!requirements.contains_module(owner));
    assert!(requirements.type_reference(&signal).is_ok());
    assert!(requirements.trait_reference(&read).is_ok());
    assert!(requirements.type_reference(&unused).is_err());
    assert!(requirements.satisfied_by(&catalog).unwrap());
    assert!(
        !requirements
            .satisfied_by(&DeclarationCatalog::default())
            .unwrap()
    );
    let seed = DeclarationCatalog::from_modules(&[&modules[0]]).unwrap();
    assert!(seed.contains_module(owner));
    assert!(seed.type_reference(&unused).is_ok());
    assert_eq!(
        seed.authored_origin(owner),
        Some(program.product_identity())
    );
    let mut later = ModuleBuilder::new("later", &seed).unwrap();
    let inspect = later
        .define_function(
            FunctionDecl::new("inspect").parameter(
                "value",
                seed.type_reference(&signal)
                    .unwrap()
                    .apply([Type::i32()])
                    .unwrap(),
            ),
        )
        .unwrap();
    later
        .bind(
            inspect,
            NativeBinding::new(vec![Codec::Value], Codec::Value, |_| {
                panic!("linking does not call Rust")
            }),
        )
        .unwrap();
    let later = later.finish().unwrap();
    assert!(
        later
            .required_source_trait_contract(&read)
            .unwrap_err()
            .to_string()
            .contains("not a mandatory source trait")
    );
    let mut runtime = Runtime::default();
    modules[0].install(&mut runtime).unwrap();
    assert!(!runtime.native_entries.catalog.contains_module(owner));
    assert!(
        runtime
            .native_entries
            .catalog
            .type_reference(&signal)
            .is_err()
    );
    assert!(
        runtime
            .native_entries
            .catalog
            .trait_reference(&read)
            .is_err()
    );
    assert!(runtime.native_entries.owned_traits.is_empty());
    for (case, diagnostic) in [
        (Product::Sibling, "requires reachable authored owner"),
        (
            Product::ExtraDefault,
            "requires the exact public authored trait",
        ),
        (
            Product::ChangedPayload,
            "requires the exact public authored type",
        ),
        (
            Product::PrivateTrait,
            "requires the exact public authored trait",
        ),
    ] {
        let product = admission_product(&program, &facade, case, None);
        let error = runtime
            .load_verified_program("facade", product)
            .unwrap_err();
        assert!(error.to_string().contains(diagnostic), "{error}");
        assert_eq!(runtime.modules().loaded_count(), 0);
    }
    // The private ABI is a valid additional linked requirement, but not a public provider.
    let mut unregistered = Runtime::default();
    ModuleBuilder::from_declaration(
        DefinitionMetadata::checked(
            catalog.definitions(),
            ModuleDecl::new(facade.clone()),
            &Default::default(),
        )
        .unwrap(),
        &Default::default(),
        Default::default(),
    )
    .unwrap()
    .finish()
    .unwrap()
    .install(&mut unregistered)
    .unwrap();
    unregistered
        .load_verified_program(
            "inferred",
            admission_product(&program, &facade, Product::PrivateTrait, None),
        )
        .unwrap();
    // An explicit pin without an impl, export, binding or call has the same obligation.
    let mut pin_only = ModuleBuilder::from_declaration(
        DefinitionMetadata::checked(
            catalog.definitions(),
            ModuleDecl::new(facade.clone()),
            &Default::default(),
        )
        .unwrap(),
        &catalog,
        Default::default(),
    )
    .unwrap();
    pin_only.reference_trait(&read).unwrap();
    assert!(pin_only.reference_trait(&read).is_err());
    let pin_only = pin_only.finish().unwrap();
    assert_eq!(
        pin_only.required_source_traits().iter().collect::<Vec<_>>(),
        [&read]
    );
    let expected = pin_only
        .required_source_trait_contract(&read)
        .unwrap()
        .import_into(catalog.types.context(), &Default::default())
        .unwrap();
    assert_eq!(expected.records(), interface.contract.as_ref());
    let mut pinned_runtime = Runtime::default();
    pin_only.install(&mut pinned_runtime).unwrap();
    let omitted = admission_product(&program, &facade, Product::Omitted, None);
    let error = pinned_runtime
        .load_verified_program("pin-only", omitted)
        .unwrap_err();
    assert!(
        error.to_string().contains("source trait references differ"),
        "{error}"
    );
    let valid = admission_product(&program, &facade, Product::Matching, None);
    pinned_runtime
        .load_verified_program("pin-only", valid.clone())
        .unwrap();
    runtime.load_verified_program("facade", valid).unwrap();

    let product = admission_product(&program, &facade, Product::Matching, Some(&later));
    let mut runtime = Runtime::default();
    modules[0].install(&mut runtime).unwrap();
    let error = runtime
        .load_verified_program("later", product.clone())
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("native declaration is not installed"),
        "{error}"
    );
    assert_eq!(runtime.modules().loaded_count(), 0);
    later.install(&mut runtime).unwrap();
    runtime.load_verified_program("later", product).unwrap();

    let mut twin = ModuleDecl::new(owner.clone());
    twin.types.push(ty.declaration.as_ref().clone());
    twin.traits.push(interface.contract.as_ref().clone());
    let twin =
        DefinitionMetadata::checked(ty.definitions().clone(), twin, &Default::default()).unwrap();
    let native = DeclarationCatalog::from_declarations([&twin]).unwrap();
    let twin_module =
        ModuleBuilder::from_declaration(twin, &Default::default(), Default::default())
            .unwrap()
            .finish()
            .unwrap();
    for (first, collision) in [(&modules[0], &twin_module), (&twin_module, &modules[0])] {
        let mut runtime = Runtime::default();
        first.install(&mut runtime).unwrap();
        let error = collision.install(&mut runtime).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("conflicting verified/native module origin"),
            "{error}"
        );
    }
    assert!(
        !requirements.satisfied_by(&native).unwrap(),
        "ABI-identical native owner is not authored evidence"
    );
    assert!(
        !native.satisfied_by(&catalog).unwrap(),
        "authored evidence cannot stand in for native ownership"
    );
    assert!(
        native
            .with_verified_modules(&program, [owner])
            .unwrap_err()
            .to_string()
            .contains("conflicting verified/native module origin")
    );

    let other = compiled(include_str!(
        "../../../tests/fixtures/execution/interface-methods.kgr"
    ));
    let mut runtime = Runtime::default();
    modules[0].install(&mut runtime).unwrap();
    runtime
        .load_verified_program("no-importer", VerifiedProgram::from_bytecode(other.clone()))
        .unwrap();
    assert!(
        catalog
            .with_verified_modules(&other, [owner])
            .unwrap_err()
            .to_string()
            .contains("conflicting verified/native module origin")
    );
}

enum Product {
    Matching,
    Omitted,
    ChangedPayload,
    ExtraDefault,
    PrivateTrait,
    Sibling,
}

fn admission_product(
    source: &VerifiedBytecodeProgram,
    facade: &ModuleIdentity,
    case: Product,
    later: Option<&NativeModule>,
) -> VerifiedProgram {
    let context = DefinitionContext::new().unwrap();
    let original = &source.program().modules[source.program().root.index()];
    let public = DefinitionMetadata::checked(
        source.definitions().clone(),
        original.public_items.clone(),
        &Default::default(),
    )
    .unwrap()
    .import_into(&context, &Default::default())
    .unwrap();
    let mut owner = BytecodeModule {
        identity: original.identity.clone(),
        public_items: public.into_records(),
        ..Default::default()
    };
    // The seed retains Envelope for future authors; this product owes neither importer that ABI.
    owner.public_items.retain(|item| item.name() != "Envelope");
    match case {
        Product::ChangedPayload => {
            let ty = owner
                .public_items
                .iter_mut()
                .find_map(|item| match item {
                    PublicItem::Type(ty) if ty.name == "Signal" => Some(ty),
                    _ => None,
                })
                .unwrap();
            ty.variants[0].payload[0] = Type::bool().abi().clone();
        }
        Product::ExtraDefault => {
            let read = owner
                .public_items
                .iter_mut()
                .find_map(|item| match item {
                    PublicItem::Trait(abi) if abi.name == "Read" => Some(abi),
                    _ => None,
                })
                .unwrap();
            let mut extra = read.methods[0].clone();
            extra.name = "extra".into();
            extra.implementation = CallableImplementation::Script;
            read.methods.push(extra);
        }
        Product::PrivateTrait => {
            let index = owner
                .public_items
                .iter()
                .position(|item| matches!(item, PublicItem::Trait(tr) if tr.name == "Read"))
                .unwrap();
            let PublicItem::Trait(abi) = owner.public_items.remove(index) else {
                unreachable!()
            };
            let declaration = context
                .intern(&path(&owner.identity, DefinitionKind::Trait, "Read"))
                .unwrap();
            owner
                .trait_contracts
                .push(TraitContract { declaration, abi });
        }
        _ => {}
    }
    let inferred = if matches!(case, Product::PrivateTrait) {
        owner.trait_contracts.clone()
    } else if matches!(case, Product::Omitted | Product::Sibling) {
        vec![]
    } else {
        owner
            .public_items
            .iter()
            .filter_map(|item| match item {
                PublicItem::Trait(abi) => Some(TraitContract {
                    declaration: context
                        .intern(&path(&owner.identity, DefinitionKind::Trait, &abi.name))
                        .unwrap(),
                    abi: abi.clone(),
                }),
                _ => None,
            })
            .collect()
    };
    // The root reaches both branches even when the installed facade cannot reach the owner.
    let mut modules = vec![
        BytecodeModule {
            identity: ModuleIdentity::single_file("root"),
            dependencies: vec![ModuleRef::new(1), ModuleRef::new(2)],
            ..Default::default()
        },
        BytecodeModule {
            identity: facade.clone(),
            trait_references: inferred,
            dependencies: if matches!(case, Product::Sibling) {
                vec![]
            } else {
                vec![ModuleRef::new(2)]
            },
            ..Default::default()
        },
        BytecodeModule {
            identity: ModuleIdentity::single_file("intermediate"),
            dependencies: vec![ModuleRef::new(3)],
            ..Default::default()
        },
        owner,
    ];
    if let Some(later) = later {
        let mut binding = free_function_program(&later.to_declaration())
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records()
            .modules
            .remove(0);
        binding.dependencies.push(ModuleRef::new(3));
        let slot = ModuleRef::new(modules.len());
        modules[0].dependencies.push(slot);
        modules.push(binding);
    }
    let program = ProgramFixture {
        root: ModuleRef::new(0),
        modules,
        exports: Default::default(),
    }
    .build(&context);
    VerifiedProgram::from_bytecode(
        VerifiedBytecodeProgram::new(program, &context, &Default::default()).unwrap(),
    )
}

#[test]
fn packaged_range_impl_proves_a_native_result_without_installing_source_as_native() {
    let cancel = Default::default();
    let (seed, code) = {
        let mut sources = SourceDatabase::default();
        let root = sources
            .set(
                "ranges.kgr",
                include_str!("../../../tests/fixtures/execution/authored-range-bounds.kgr").into(),
                SourceLayer::Base,
            )
            .unwrap();
        let mut db = AnalysisDatabase::default();
        let snapshot = db.snapshot(sources.snapshot(), &cancel).unwrap();
        let checked = snapshot.check_program(root, &cancel).unwrap();
        let input = CheckedCompilation::from_program(&checked, &cancel).unwrap();
        let mir = lower_program_to_mir(&input, &Default::default()).unwrap();
        let code = lower_program_to_bytecode(&mir, &cancel).unwrap();
        let seed = DeclarationCatalog::default()
            .with_verified_modules(
                &code,
                code.program()
                    .modules
                    .iter()
                    .map(|m| &m.identity)
                    .filter(|id| id.package.0 == "kagari-core"),
            )
            .unwrap();
        (seed, code)
    };
    let convert = Protocol::From.module();
    let forward = seed
        .trait_reference(&path(&convert, DefinitionKind::Trait, "TryFrom"))
        .unwrap();
    let adapter = forward.contract.conversion_adapter.as_ref().unwrap();
    assert_eq!(adapter.method, forward.method("try_from").unwrap().id());
    let ops = Protocol::Add.module();
    let bounds = seed
        .trait_reference(&path(&ops, DefinitionKind::Trait, "RangeBounds"))
        .unwrap()
        .apply([Type::i32()])
        .unwrap();
    let range = seed
        .type_reference(&path(
            &ops,
            DefinitionKind::AssociatedType,
            "RangeInclusive",
        ))
        .unwrap();
    let importer = code.program().modules[code.program().root.index()]
        .identity
        .clone();
    let record = DefinitionMetadata::checked(
        seed.definitions(),
        ModuleDecl::new(importer.clone()),
        &cancel,
    )
    .unwrap();
    let mut factory = ModuleBuilder::from_declaration(record, &seed, Default::default()).unwrap();
    let create = factory
        .define_function(FunctionDecl::new("make").returns(bounds.ty()))
        .unwrap();
    let value = range.apply([Type::i32()]).unwrap();
    assert!(
        matches!(value.abi(), Ty::Range(element, RangeKind::Inclusive) if **element == *Type::i32().abi())
    );
    factory.function(&create, |f| f.produces(value)).unwrap();
    factory
        .bind(
            create,
            NativeBinding::new(vec![], Codec::Value, |_| {
                panic!("preparation is not execution")
            }),
        )
        .unwrap();
    let module = factory.finish().unwrap();
    assert_eq!(module.catalog().source_impls.len(), 1);
    assert!(seed.source_impls.len() > 1);
    let seed_tables = Arc::downgrade(&module.authored_seed.as_ref().unwrap().source_impls);
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    assert!(runtime.native_entries.catalog.source_impls.is_empty());
    assert!(runtime.native_entries.catalog.implementations.is_empty());
    // All nominal declarations still match, but this independently verified owner
    // has no RangeInclusive implementation. Registering the Rust entry cannot supply it.
    let required = module.catalog().authored_contracts();
    let context = required.types.context();
    let input = ProgramFixture {
        root: ModuleRef::new(0),
        exports: Default::default(),
        modules: vec![
            BytecodeModule {
                identity: importer,
                dependencies: vec![ModuleRef::new(1)],
                ..Default::default()
            },
            BytecodeModule {
                identity: ops,
                public_items: required
                    .types
                    .values()
                    .cloned()
                    .map(PublicItem::Type)
                    .chain(required.traits.values().cloned().map(PublicItem::Trait))
                    .collect(),
                ..Default::default()
            },
        ],
    }
    .build(context);
    let missing = VerifiedBytecodeProgram::new(input, context, &cancel).unwrap();
    let error = runtime
        .load_verified_program("missing impl", VerifiedProgram::from_bytecode(missing))
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires the exact authored implementation"),
        "{error}"
    );
    assert_eq!(runtime.modules().loaded_count(), 0);
    drop(module);
    drop(seed);
    assert!(
        seed_tables.upgrade().is_none(),
        "installed requirements retained the complete proof seed"
    );
    let publication = code.product_identity();
    let loaded = runtime
        .load_verified_program("matching impl", VerifiedProgram::from_bytecode(code))
        .unwrap();
    let selected = runtime.select_program(&loaded).unwrap();
    for path in [binding::option_declaration(), binding::result_declaration()] {
        let view = selected.conversion_catalog();
        view.type_reference(&path).unwrap();
        assert_eq!(view.authored_origin(&path.module), Some(publication));
        assert!(!view.contains_module(&path.module));
        assert!(
            runtime
                .native_entries
                .catalog
                .type_reference(&path)
                .is_err()
        );
    }
}

#[test]
fn verified_host_schema_is_not_an_authored_nominal_contract() {
    let context = DefinitionContext::new().unwrap();
    let owner = ModuleIdentity::single_file("external");
    let declaration = context
        .intern(&path(&owner, DefinitionKind::Struct, "External"))
        .unwrap();
    let definition = TypeDef {
        name: "External".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadWrite,
        }),
        generic_params: vec![],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    };
    let program = ProgramFixture {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            identity: owner.clone(),
            public_items: vec![PublicItem::Type(definition.clone())],
            external_types: vec![TypeDeclaration {
                declaration,
                definition,
            }],
            ..Default::default()
        }],
        exports: Default::default(),
    }
    .build(&context);
    let program = VerifiedBytecodeProgram::new(program, &context, &Default::default()).unwrap();
    let error = DeclarationCatalog::default()
        .with_verified_modules(&program, [&owner])
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("verified public contract requires native authority"),
        "{error}"
    );
}

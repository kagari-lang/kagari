//! Match required registration authority against the verified executable closure.
use crate::{
    error::RuntimeError,
    module::{VerifiedProgram, admission::DefinitionAdmission},
    native::{catalog::DeclarationCatalog, registry::matching},
};
use kagari_bytecode::module::BytecodeModule;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity,
    table::{DefinitionId, DefinitionTable},
};
use kagari_contract::types::{
    InterfaceTable, PublicItem, inherent::InherentTable, trait_contract, type_contract,
};
use kagari_types::{
    declaration::{
        TraitDef, TypeDefKind, conversion::is_library_forwarding_trait, module::ImplDecl,
        native::NativeTypeConstructor,
    },
    language::{
        Protocol,
        binding::future_declaration,
        comparison, requirements,
        schema::{CoreEnum, constructor_shape_valid, future_shape_valid},
    },
    range::RangeKind,
    ty::Ty,
};
use std::collections::{BTreeMap, HashSet};

/// Only follow the requesting module's verified edges. A layout in that module,
/// or a declaration in another branch of the program, is not an imported owner.
fn reachable_owner<'a>(
    module: &BytecodeModule,
    owner: &ModuleIdentity,
    program: &'a VerifiedProgram,
) -> Option<&'a BytecodeModule> {
    let mut seen = HashSet::new();
    let mut pending = module.dependencies.clone();
    while let Some(next) = pending.pop() {
        if seen.insert(next) {
            let dependency = &program.modules()[next.index()];
            if &dependency.identity == owner {
                return Some(dependency);
            }
            pending.extend_from_slice(&dependency.dependencies);
        }
    }
    None
}

fn public_enum_valid(
    required: CoreEnum,
    module: &BytecodeModule,
    source: &DefinitionTable,
) -> bool {
    source
        .lookup(&required.declaration())
        .and_then(|owner| {
            type_contract(&module.identity, &module.public_items, &owner, source)
                .map(|record| required.shape_valid(owner, record, source))
        })
        .unwrap_or(false)
}

/// Registered value/external schemas remain exact even without callable imports.
pub(super) fn validate_installed_values(
    installed: &DeclarationCatalog,
    module: &BytecodeModule,
    program: &VerifiedProgram,
    admission: &DefinitionAdmission,
) -> Result<(), RuntimeError> {
    let definitions = installed.definitions();
    let source = admission.source();
    let future = future_declaration();
    if module.identity == future.module
        && !source.lookup(&future).is_some_and(|id| {
            type_contract(&module.identity, &module.public_items, &id, source)
                .is_some_and(|ty| future_shape_valid(id, ty, source))
        })
    {
        return Err(RuntimeError::module_validation(
            "missing or invalid public core Future declaration",
        ));
    }
    if module.identity == Protocol::PartialOrd.module()
        && reachable_owner(module, &CoreEnum::Option.declaration().module, program).is_none()
    {
        return Err(RuntimeError::module_validation(
            "comparison requires the reachable public Option declaration",
        ));
    }
    // Every reached owner is checked on its own module, including Option when
    // loaded without cmp. Read product records only in the product's table.
    let authored = !installed.contains_module(&module.identity);
    if authored {
        if module.identity == Protocol::From.module() {
            let required = CoreEnum::Result;
            let path = required.declaration();
            let owner = reachable_owner(module, &path.module, program).ok_or_else(|| {
                RuntimeError::module_validation(format!(
                    "{} requires the reachable {required:?} owner",
                    module.identity
                ))
            })?;
            // An installed owner may legitimately be partial. When another module
            // uses the source operation, its signature still needs this public type.
            if installed.contains_module(&owner.identity)
                && (installed.types.get(&path).is_none()
                    || !public_enum_valid(required, owner, source))
            {
                return Err(RuntimeError::module_validation(format!(
                    "installed dependency lacks the required public {required:?} schema"
                )));
            }
        }
        if module.identity == Protocol::Try.module() {
            for kind in RangeKind::ALL {
                validate_constructor(NativeTypeConstructor::Range(kind), module, source)?;
            }
        } else if module.identity == NativeTypeConstructor::String.declaration().module {
            validate_constructor(NativeTypeConstructor::String, module, source)?;
        }
    }
    for required in CoreEnum::COMPARISON
        .into_iter()
        .chain(CoreEnum::OPERATIONS.into_iter().filter(|_| authored))
    {
        let path = required.declaration();
        if path.module != module.identity {
            continue;
        }
        if installed.contains_module(&path.module) && installed.types.get(&path).is_none() {
            return Err(RuntimeError::module_validation(format!(
                "installed owner lacks the required comparison enum in {}",
                path.module
            )));
        }
        if !public_enum_valid(required, module, source) {
            return Err(RuntimeError::module_validation(format!(
                "missing or invalid public {} enum {required:?} in {}",
                if CoreEnum::COMPARISON.contains(&required) {
                    "comparison"
                } else {
                    "operation"
                },
                module.identity
            )));
        }
    }
    for (id, expected) in installed.types.entries().filter(|(_, ty)| {
        matches!(
            ty.kind,
            TypeDefKind::Struct | TypeDefKind::Enum | TypeDefKind::Host(_)
        )
    }) {
        let owner = definitions
            .resolve(id)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        if owner.module() != &module.identity {
            continue;
        }
        let actual = source.lookup(&owner.to_path()).and_then(|product| {
            type_contract(&module.identity, &module.public_items, &product, source)
        });
        if actual
            .map(|record| admission.runtime(record, &Default::default()))
            .transpose()?
            .as_ref()
            != Some(expected)
        {
            return Err(RuntimeError::module_validation(
                "registered value schema differs from its installed contract",
            ));
        }
    }
    for required in &module.external_types {
        let required = admission.runtime(required, &Default::default())?;
        if installed.types.get_id(required.declaration) != Some(&required.definition) {
            return Err(RuntimeError::module_validation(
                "external type dependency lacks its exact installed contract",
            ));
        }
    }
    Ok(())
}

fn validate_constructor(
    constructor: NativeTypeConstructor,
    module: &BytecodeModule,
    source: &DefinitionTable,
) -> Result<(), RuntimeError> {
    let path = constructor.declaration();
    if !source.lookup(&path).is_some_and(|owner| {
        type_contract(&module.identity, &module.public_items, &owner, source)
            .is_some_and(|record| constructor_shape_valid(owner, record, source))
    }) {
        return Err(RuntimeError::module_validation(format!(
            "missing or invalid public language constructor: {constructor:?}"
        )));
    }
    Ok(())
}

/// Installed trait identities retain their exact contracts even without calls.
/// Without an installed owner, supported public core traits must prove their operation
/// contracts. This never authorizes a native function or storage provider.
pub(super) fn validate_installed_traits(
    installed: &DeclarationCatalog,
    module: &BytecodeModule,
    admission: &DefinitionAdmission,
) -> Result<(), RuntimeError> {
    let definitions = installed.definitions();
    let source = admission.source();
    let authored = !installed.contains_module(&module.identity);
    let mut companions = if authored {
        requirements::companions(&module.identity).to_vec()
    } else {
        vec![]
    };
    let mut missing: Vec<_> = Protocol::ALL
        .into_iter()
        .filter(|protocol| protocol.module() == module.identity)
        .collect();
    for item in &module.public_items {
        let PublicItem::Trait(contract) = item else {
            continue;
        };

        let lexical = source
            .lookup_root(&module.identity)
            .and_then(|root| source.lookup_child(root, DefinitionKind::Trait, &contract.name, 0));
        let role = missing
            .iter()
            .position(|protocol| protocol.name() == contract.name);
        let id = definitions.lookup_root(&module.identity).and_then(|root| {
            definitions.lookup_child(root, DefinitionKind::Trait, &contract.name, 0)
        });
        let comparison_role = role.is_some_and(|index| missing[index].comparison());
        if comparison_role
            && !lexical.is_some_and(|id| comparison::trait_shape_valid(id, contract, source))
        {
            return Err(RuntimeError::module_validation(format!(
                "invalid public comparison trait: {}",
                contract.name
            )));
        }
        let companion = companions.iter().position(|name| *name == contract.name);
        let operation = authored
            && (role.is_some_and(|i| missing[i].source_operation()) || companion.is_some());
        if operation
            && !lexical.is_some_and(|id| requirements::trait_shape_valid(id, contract, source))
        {
            return Err(RuntimeError::module_validation(format!(
                "invalid public operation trait: {}",
                contract.name
            )));
        }
        let valid = if let Some(id) = id
            && let Some(expected) = installed.traits.get_id(id)
        {
            let actual = admission.runtime(contract, &Default::default())?;
            matching::trait_contract(id, expected, &actual, &definitions)
        } else if role.is_some() || companion.is_some() {
            operation || (authored && comparison_role)
        } else {
            contract.conversion_adapter.is_none()
                && !lexical.is_some_and(|id| is_library_forwarding_trait(source, id))
        };
        if !valid {
            return Err(RuntimeError::module_validation(format!(
                "reserved or native trait differs from its installed contract: {}",
                contract.name
            )));
        }
        if let Some(index) = companion {
            companions.swap_remove(index);
        }
        if let Some(index) = role {
            missing.swap_remove(index);
        }
    }
    for contract in &module.trait_contracts {
        let id = admission.runtime_id(contract.declaration)?;
        let valid = if let Some(expected) = installed.traits.get_id(id) {
            matching::trait_contract(
                id,
                expected,
                &admission.runtime(&contract.abi, &Default::default())?,
                &definitions,
            )
        } else {
            Protocol::from_definition(&contract.declaration, source).is_none()
                && !is_library_forwarding_trait(source, contract.declaration)
                && contract.abi.conversion_adapter.is_none()
                && !(authored
                    && requirements::companions(&module.identity)
                        .contains(&contract.abi.name.as_str())
                    && source.lookup_root(&module.identity).and_then(|root| {
                        source.lookup_child(root, DefinitionKind::Trait, &contract.abi.name, 0)
                    }) == Some(contract.declaration))
        };
        if !valid {
            return Err(RuntimeError::module_validation(
                "private reserved or native trait lacks an installed contract",
            ));
        }
    }
    if !missing.is_empty() || !companions.is_empty() {
        return Err(RuntimeError::module_validation(
            "language foundation is missing a required role",
        ));
    }
    Ok(())
}

/// Linked verification already matched every unique carried reference to its reachable
/// owner. The compiler may add private interface requirements beyond this installed subset.
pub(super) fn validate_expected_traits(
    expected: &BTreeMap<DefinitionId, TraitDef>,
    module: &BytecodeModule,
    admission: &DefinitionAdmission,
) -> Result<(), RuntimeError> {
    let mut matched = 0;
    for reference in &module.trait_references {
        if let Some(required) = expected.get(&admission.runtime_id(reference.declaration)?)
            && &admission.runtime(&reference.abi, &Default::default())? == required
        {
            matched += 1;
        }
    }
    if matched != expected.len() {
        return Err(RuntimeError::module_validation(
            "source trait references differ from their installed module contracts",
        ));
    }
    Ok(())
}

pub(super) fn validate(
    required: &DeclarationCatalog,
    program: &VerifiedProgram,
    admission: &DefinitionAdmission,
) -> Result<(), RuntimeError> {
    // Select the exact identity (or identity-bearing type shape) before adopting
    // a candidate contract. Equality remains authoritative; scanning must not
    // repeatedly remap unrelated declarations for every installed binding.
    let definitions = required.definitions();
    for (id, expected) in required.types.entries() {
        let owner = definitions
            .resolve(id)
            .map_err(|cause| RuntimeError::module_validation(cause.to_string()))?;
        if !program.modules().iter().any(|module| {
            &module.identity == owner.module()
                && module.public_items.iter().any(
                    |item| matches!(item, PublicItem::Type(declaration) if declaration.name == expected.name && declaration.kind == expected.kind && admission.runtime(declaration, &Default::default()).as_ref().is_ok_and(|actual| actual == expected)),
                )
        }) {
            return Err(RuntimeError::module_validation(format!(
                "native type {} differs from its registered contract",
                expected.name
            )));
        }
    }
    for (id, expected) in required.traits.entries() {
        let owner = definitions
            .resolve(id)
            .map_err(|cause| RuntimeError::module_validation(cause.to_string()))?;
        if !program.modules().iter().any(|module| {
            &module.identity == owner.module()
                && trait_contract(
                    &module.identity,
                    &module.public_items,
                    &module.trait_contracts,
                    &match admission.product_id(id) {
                        Some(id) => id,
                        None => return false,
                    },
                    program.definitions(),
                )
                .is_some_and(|actual| {
                    admission
                        .runtime(actual, &Default::default())
                        .as_ref()
                        .is_ok_and(|actual| {
                            matching::trait_contract(id, expected, actual, &definitions)
                        })
                })
        }) {
            return Err(RuntimeError::module_validation(format!(
                "native dependency {} differs from its registered trait contract",
                expected.name
            )));
        }
    }
    for (id, expected) in required.declarations.entries() {
        let owner = definitions
            .resolve(id)
            .map_err(|cause| RuntimeError::module_validation(cause.to_string()))?;
        if !program.modules().iter().any(|module| {
            &module.identity == owner.module()
                && module.native_declarations.iter().any(|declaration| {
                    if admission.runtime_id(declaration.declaration).ok() != Some(id) {
                        return false;
                    }
                    admission
                        .runtime(declaration, &Default::default())
                        .as_ref()
                        .is_ok_and(|actual| matching::declaration(expected, actual, &definitions))
                })
        }) {
            return Err(RuntimeError::module_validation(
                "native dependency differs from its registered template contract",
            ));
        }
    }
    for (id, expected) in required.implementations.entries() {
        let owner = definitions
            .resolve(id)
            .map_err(|cause| RuntimeError::module_validation(cause.to_string()))?;
        if !program
            .modules()
            .iter()
            .filter(|module| &module.identity == owner.module())
            .flat_map(|module| &module.public_items)
            .any(|item| match item {
                PublicItem::InterfaceTable(actual)
                    if admission.runtime_id(actual.declaration).ok() == Some(id) =>
                {
                    admission
                        .runtime(actual.as_ref(), &Default::default())
                        .as_ref()
                        .is_ok_and(|actual| {
                            implementation_matches(&id, expected, actual, &definitions)
                        })
                }
                PublicItem::InherentTable(actual)
                    if admission.runtime_id(actual.declaration).ok() == Some(id) =>
                {
                    admission
                        .runtime(actual.as_ref(), &Default::default())
                        .as_ref()
                        .is_ok_and(|actual| inherent_matches(id, expected, actual, &definitions))
                }
                _ => false,
            })
        {
            return Err(RuntimeError::module_validation(
                "native dependency differs from its registered implementation contract",
            ));
        }
    }
    Ok(())
}

pub(super) fn implementation_matches(
    id: &DefinitionId,
    expected: &ImplDecl,
    actual: &InterfaceTable,
    definitions: &DefinitionTable,
) -> bool {
    // The compiler materializes omitted defaults on its real executable table.
    // Match the registered header and every explicit native method; defaults are
    // independently checked against the exact carried trait/template contracts.
    actual.declaration == *id
        && actual.bridge.is_none()
        && actual.generic_params == expected.generic_params
        && actual.bounds == expected.bounds
        && actual.for_type == expected.for_type
        && actual.associated_type_families.is_empty()
        && actual.associated_consts.is_empty()
        && matches!(&actual.trait_type, Ty::Trait(interface) if Some(interface) == expected.trait_type.as_ref())
        && expected.methods.iter().all(|method| {
            let mut method = method.clone();
            method
                .generic_params
                .retain(|parameter| !expected.generic_params.contains(parameter));
            matching::method(*id, &method, definitions)
                .is_some_and(|expected| actual.methods.contains(&expected))
        })
}

fn inherent_matches(
    id: DefinitionId,
    expected: &ImplDecl,
    actual: &InherentTable,
    definitions: &DefinitionTable,
) -> bool {
    expected.trait_type.is_none()
        && actual.declaration == id
        && actual.generic_params == expected.generic_params
        && actual.bounds == expected.bounds
        && actual.for_type == expected.for_type
        && expected.methods.iter().all(|method| {
            let mut method = method.clone();
            method
                .generic_params
                .retain(|parameter| !expected.generic_params.contains(parameter));
            matching::method(id, &method, definitions)
                .is_some_and(|expected| actual.methods.contains(&expected))
        })
}

#[cfg(test)]
mod tests {
    use crate::test_support::bytecode::load_bytecode;
    use crate::test_support::native::program::ProgramFixture;
    use crate::{Runtime, RuntimeConfig, module::VerifiedProgram};
    use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
    use kagari_common::identity::{
        DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
        metadata::DefinitionMetadata, table::DefinitionId,
    };
    use kagari_compiler::source::lower::module_contract;
    use kagari_contract::{
        layout::{EnumLayout, EnumVariantLayout},
        types::{PublicItem, type_contract},
    };
    use kagari_hir::{
        analysis::{AnalysisDatabase, core_source::packaged_modules},
        compile_input::program::CheckedCompilation,
    };
    use kagari_types::{
        declaration::{
            TypeDeclaration, TypeDef, TypeDefKind, VariantDef,
            host::{HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess},
            module::ModuleDecl,
        },
        language::{Protocol, binding},
        ty::{GenericParam, NominalTy, Ty},
    };

    fn option_schema(owner: DefinitionId) -> TypeDef {
        let parameter = GenericParam { owner, position: 0 };
        TypeDef {
            name: "Option".into(),
            kind: TypeDefKind::Enum,
            variants: vec![
                VariantDef {
                    name: "Some".into(),
                    reports_failure: false,
                    payload: vec![parameter.as_type()],
                },
                VariantDef {
                    name: "None".into(),
                    reports_failure: false,
                    payload: vec![],
                },
            ],
            generic_params: vec![parameter],
            bounds: vec![],
            fields: vec![],
        }
    }

    #[test]
    fn source_option_requires_a_public_schema_with_the_canonical_payload() {
        let context = DefinitionContext::new().unwrap();
        let path = binding::option_declaration();
        let owner = context.intern(&path).unwrap();
        let mut option = option_schema(owner);
        let load = |public_items, private_types| {
            let program = ProgramFixture {
                root: ModuleRef::new(0),
                exports: Default::default(),
                modules: vec![BytecodeModule {
                    identity: path.module.clone(),
                    public_items,
                    private_types,
                    ..Default::default()
                }],
            }
            .build(&context);
            let input =
                DefinitionMetadata::checked(context.snapshot(), program, &Default::default())
                    .unwrap();
            load_bytecode(&mut Runtime::default(), "option", input)
        };
        load(vec![PublicItem::Type(option.clone())], vec![]).unwrap();
        let hidden = TypeDeclaration {
            declaration: owner,
            definition: option.clone(),
        };
        let error = load(vec![], vec![hidden]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("missing or invalid public comparison enum"),
            "{error}"
        );
        option.variants[0].payload.clear();
        let error = load(vec![PublicItem::Type(option)], vec![]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("missing or invalid public comparison enum"),
            "{error}"
        );
    }

    fn comparison_fixture(context: &DefinitionContext, option: DefinitionId) -> BytecodeModule {
        let cancel = Default::default();
        let sources = packaged_modules();
        let identity = Protocol::Ord.module();
        let cmp = sources
            .iter()
            .find(|m| m.source.module_identity() == &identity)
            .unwrap();
        let mut db = AnalysisDatabase::default();
        let snapshot = db.snapshot(Default::default(), &cancel).unwrap();
        let checked = snapshot.check_program(cmp.source.id(), &cancel).unwrap();
        let input = CheckedCompilation::from_program(&checked, &cancel).unwrap();
        let items = module_contract(input.module(input.root()).unwrap(), &cancel)
            .unwrap()
            .import_into(context, &cancel)
            .unwrap()
            .into_records()
            .public_items;
        let ordering = context.intern(&binding::ordering_declaration()).unwrap();
        let variants = &type_contract(&identity, &items, &ordering, &context.snapshot())
            .unwrap()
            .variants;
        let order = Ty::Enum(NominalTy {
            declaration: ordering,
            arguments: vec![],
            associated_types: Default::default(),
        });
        let layout = |declaration, arguments, variants: &[VariantDef]| EnumLayout {
            declaration,
            arguments,
            variants: variants
                .iter()
                .map(|v| EnumVariantLayout {
                    declaration: context
                        .intern_child(declaration, DefinitionKind::Variant, &v.name, 0)
                        .unwrap(),
                    reports_failure: v.reports_failure,
                    payload: v.payload.clone(),
                })
                .collect(),
        };
        let mut option_variants = option_schema(option).variants;
        option_variants[0].payload = vec![order.clone()];
        let enumerations = vec![
            layout(ordering, vec![], variants),
            layout(option, vec![order], &option_variants),
        ];
        BytecodeModule {
            identity,
            public_items: items,
            enumerations,
            ..Default::default()
        }
    }

    #[test]
    fn comparison_requires_option_in_its_own_dependency_closure() {
        let context = DefinitionContext::new().unwrap();
        let path = binding::option_declaration();
        let option = context.intern(&path).unwrap();
        let mut cmp = comparison_fixture(&context, option);
        cmp.dependencies.push(ModuleRef::new(2));
        let mut modules = vec![
            BytecodeModule {
                identity: ModuleIdentity::single_file("consumer"),
                dependencies: vec![ModuleRef::new(1), ModuleRef::new(3)],
                ..Default::default()
            },
            cmp,
            BytecodeModule {
                identity: ModuleIdentity::single_file("bridge"),
                dependencies: vec![ModuleRef::new(3)],
                ..Default::default()
            },
            BytecodeModule {
                identity: path.module,
                public_items: vec![PublicItem::Type(option_schema(option))],
                ..Default::default()
            },
        ];
        let load = |modules| {
            let program = ProgramFixture {
                root: ModuleRef::new(0),
                modules,
                exports: Default::default(),
            }
            .build(&context);
            let metadata =
                DefinitionMetadata::checked(context.snapshot(), program, &Default::default())
                    .unwrap();
            let verified =
                VerifiedProgram::new(metadata, &DefinitionContext::new().unwrap()).unwrap();
            Runtime::default().load_verified_program("comparison", verified)
        };
        load(modules.clone()).unwrap();
        // The root still reaches both owners, and cmp still carries a valid applied
        // Option<Ordering> layout. Only the importer's path to the schema is lost.
        modules[2].dependencies.clear();
        let error = load(modules).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("comparison requires the reachable public Option declaration"),
            "{error}"
        );
    }

    #[test]
    fn uninstalled_external_schema_cannot_publish_a_source_free_module() {
        let context = DefinitionContext::new().unwrap();
        let mut declaration = ModuleDecl::new(ModuleIdentity {
            package: PackageId("example".into()),
            path: vec!["roots".into()],
        });
        let definition = TypeDef {
            name: "Player".into(),
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
        declaration.types.push(definition.clone());
        declaration.collect_definitions(&context).unwrap();
        let id = declaration
            .definition(DefinitionKind::Struct, "Player", &context.snapshot())
            .unwrap();
        let program = ProgramFixture {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                identity: declaration.identity,
                public_items: vec![PublicItem::Type(definition.clone())],
                external_types: vec![TypeDeclaration {
                    declaration: id,
                    definition,
                }],
                ..Default::default()
            }],
            exports: Default::default(),
        }
        .build(&context);
        let input =
            DefinitionMetadata::checked(context.snapshot(), program, &Default::default()).unwrap();
        let mut runtime = Runtime::new(RuntimeConfig::default());
        let error = load_bytecode(&mut runtime, "external", input).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("external type dependency lacks its exact installed contract"),
            "{error}"
        );
        assert_eq!(runtime.modules().loaded_count(), 0);
    }
}

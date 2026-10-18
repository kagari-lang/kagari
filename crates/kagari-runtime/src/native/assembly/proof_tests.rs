//! Semantic proof boundaries, without constructing or installing a Runtime.
use crate::native::{
    assembly::{
        ModuleDefinition, ModuleSetBuilder,
        draft::{ModuleDraft, TraitImplementation},
    },
    binding::{Codec, NativeBinding, NativeResult},
    catalog::DeclarationCatalog,
    declarations::MethodDecl,
    types::Type,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId},
};
use kagari_types::{
    declaration::{TypeDefKind, VariantDef, native::NativeTypeConstructor},
    scalar::BuiltinType,
};

fn owner(name: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("proof-test".into()),
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

fn payload(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let payload = module.define_type("Payload", TypeDefKind::Enum, 1, |record, _| {
        record.variants.push(VariantDef {
            name: "Value".into(),
            payload: vec![record.generic_params[0].as_type()],
            reports_failure: false,
        });
        Ok(())
    })?;
    let interface =
        module.reference_trait(path("interface", DefinitionKind::Trait, "Provider"), 1)?;
    module.trait_implementation_with(1, |parameters| {
        Ok(TraitImplementation {
            receiver: payload.apply(parameters.to_vec())?,
            interface: interface.apply(parameters.to_vec())?,
            bounds: vec![],
            bindings: vec![],
        })
    })
}

fn interface(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let payload = module.reference_type(
        path("payload", DefinitionKind::Enum, "Payload"),
        TypeDefKind::Enum,
        1,
    )?;
    let contract = module.define_trait("Provider", 1, |_, _| Ok(()))?;
    module.define_trait_default(
        &contract,
        MethodDecl::instance("get")
            .returns(payload.apply([contract.parameter(0)?])?)
            .implementation_with(|view| {
                let declaration = view.declaration();
                let signature = &declaration.records().function;
                Ok(NativeBinding::new(
                    signature
                        .params
                        .iter()
                        .map(|parameter| {
                            Type::from_semantic(parameter.ty.clone(), declaration.definitions())
                                .map(Codec::Exact)
                        })
                        .collect::<NativeResult<Vec<_>>>()?,
                    Codec::Exact(Type::from_semantic(
                        signature.return_type.clone(),
                        declaration.definitions(),
                    )?),
                    |_| panic!("default body must not execute"),
                ))
            }),
    )?;
    Ok(())
}

#[test]
fn reciprocal_generic_trait_default_uses_existing_lowering_in_both_orders() {
    for reverse in [false, true] {
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        let mut recipes = vec![
            ModuleDefinition::new(owner("payload"), payload),
            ModuleDefinition::new(owner("interface"), interface),
        ];
        if reverse {
            recipes.reverse();
        }
        for recipe in recipes {
            set.add(recipe).unwrap();
        }
        let assembly = set.finish().unwrap();
        let snapshot = assembly.declarations();
        let modules = assembly.into_modules().unwrap();
        for (sealed, prepared) in snapshot.modules().iter().zip(&modules) {
            assert_eq!(
                sealed
                    .records()
                    .compiler_input_bytes(sealed.definitions(), &CancellationToken::default())
                    .unwrap(),
                prepared
                    .declaration()
                    .compiler_input_bytes(prepared.definitions(), &CancellationToken::default())
                    .unwrap()
            );
        }
        let record = snapshot
            .modules()
            .iter()
            .find(|module| module.records().identity == owner("payload"))
            .unwrap()
            .records();
        assert!(
            record.implementations[0].methods.is_empty(),
            "omitted override must retain the shared default, not synthesize a callback"
        );
        assert_eq!(record.implementations[0].generic_params.len(), 1);
    }
}

#[test]
fn concrete_result_requires_interface_evidence_before_any_factory_runs() {
    for implements in [false, true] {
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        set.add(ModuleDefinition::new(owner("split"), move |module| {
            module.define_constructor("String", NativeTypeConstructor::String)?;
            let view = module.define_trait("View", 0, |_, _| Ok(()))?;
            let data = module.define_type("Data", TypeDefKind::Enum, 0, |record, _| {
                record.variants.push(VariantDef {
                    name: "Value".into(),
                    payload: vec![],
                    reports_failure: false,
                });
                Ok(())
            })?;
            if implements {
                module.trait_implementation(data.apply([])?, view.apply([])?, vec![])?;
            }
            let concrete = data.apply([])?;
            module.inherent(Type::scalar(BuiltinType::String), |methods| {
                methods.define(
                    MethodDecl::instance("split")
                        .produces(concrete.clone())
                        .documentation("Return a declared interface backed by a concrete value.")
                        .parameter("separator", Type::scalar(BuiltinType::String))
                        .returns(view.apply([])?)
                        .implementation_with(move |_| {
                            Ok(NativeBinding::new(
                                vec![
                                    Codec::Exact(Type::scalar(BuiltinType::String)),
                                    Codec::Exact(Type::scalar(BuiltinType::String)),
                                ],
                                Codec::Exact(concrete),
                                |_| panic!("callback must not execute"),
                            ))
                        }),
                )?;
                Ok(())
            })
        }))
        .unwrap();
        let result = set.finish();
        if implements {
            let assembly = result.unwrap();
            let snapshot = assembly.declarations();
            assert_eq!(snapshot.modules()[0].records().concrete_results.len(), 1);
            assert!(assembly.into_modules().is_ok());
        } else {
            assert!(result.is_err());
        }
    }
}

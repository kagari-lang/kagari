use super::collect_module_abi;
use kagari_contract::{
    callable::CallableImplementation,
    language::{self as traits, Protocol},
    standard::surface::StandardEnum,
    types::{
        self as abi, NominalTy, PublicItem, Ty, TypeDefKind, inheritance::trait_closure,
        native::NativeTypeConstructor, verify,
    },
};
use kagari_hir::{
    aggregates::traits::MethodDefault, analysis::AnalysisDatabase,
    native::NativeBinding as HirNativeBinding,
};
use std::collections::BTreeMap;
use {
    kagari_common::identity::associated_type_id,
    kagari_source::source_database::{SourceDatabase, SourceLayer},
};

#[test]
fn installed_native_declarations_keep_public_representation_and_payload_contracts() {
    let mut sources = SourceDatabase::default();
    sources
        .set("native-abi.kgr", "fn main() {}".into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let mut count = 0;
    let mut seen_map = false;
    let mut seen_result = false;
    for declared in snapshot.declaration_snapshot().files() {
        if !matches!(
            declared.source().module_identity().package.0.as_str(),
            "kagari-core" | "kagari-alloc" | "std"
        ) {
            continue;
        }
        let analyzed = snapshot.file(declared.source().id()).unwrap();
        assert!(
            analyzed.result().diagnostics().is_empty(),
            "{}: {:?}",
            declared.source().name(),
            analyzed.result().diagnostics()
        );
        let abi = collect_module_abi(analyzed.to_unverified(&Default::default()).unwrap().facts());
        let native: Vec<_> = abi.public_items.into_iter().filter(|item| {
            matches!(item, PublicItem::Type(ty) if matches!(ty.kind, TypeDefKind::Native(_)))
        }).collect();
        verify::validate(
            &native,
            declared.source().module_identity(),
            &Default::default(),
        )
        .unwrap();
        for item in &native {
            let PublicItem::Type(ty) = item else {
                unreachable!()
            };
            assert!(ty.fields.is_empty());
            match ty.kind {
                TypeDefKind::Native(NativeTypeConstructor::Map) => {
                    assert_eq!(ty.name, "HashMap");
                    assert_eq!(ty.generic_params.len(), 2);
                    seen_map = true;
                }
                TypeDefKind::Native(NativeTypeConstructor::Enum(StandardEnum::Result)) => {
                    assert_eq!(ty.name, "Result");
                    assert_eq!(
                        ty.variants
                            .iter()
                            .map(|variant| variant.name.as_str())
                            .collect::<Vec<_>>(),
                        ["Ok", "Err"]
                    );
                    assert_eq!(ty.variants[0].payload, [ty.generic_params[0].as_type()]);
                    assert_eq!(ty.variants[1].payload, [ty.generic_params[1].as_type()]);
                    seen_result = true;
                }
                _ => {}
            }
            count += 1;
        }
    }
    assert_eq!(count, 18);
    assert!(seen_map && seen_result);
}

#[test]
fn installed_trait_contracts_and_defaults_lower_from_checked_source() {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("trait-abi.kgr", "fn main() {}".into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let root = snapshot
        .file(root)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let mut contracts = BTreeMap::new();
    for declared in snapshot.declaration_snapshot().files() {
        if declared.source().module_identity().package.0 != "kagari-core" {
            continue;
        }
        let analyzed = snapshot.file(declared.source().id()).unwrap();
        assert!(
            analyzed.result().diagnostics().is_empty(),
            "{}: {:?}",
            declared.source().name(),
            analyzed.result().diagnostics()
        );
        let module =
            collect_module_abi(analyzed.to_unverified(&Default::default()).unwrap().facts());
        for kind in Protocol::ALL {
            let id = traits::identity(kind);
            if id.module != *declared.source().module_identity() {
                continue;
            }
            let record = abi::trait_contract(
                &id.module,
                &module.public_items,
                &module.trait_contracts,
                &id,
            )
            .expect("lowered source trait");
            let source = root.facts().aggregates.trait_(&id).unwrap();
            assert_eq!(record.generic_params.len(), source.generic_params.len());
            assert_eq!(record.methods.len(), source.methods.len());
            for (slot, method) in source.methods.iter().enumerate() {
                assert_eq!(record.methods[slot].name, method.name);
                let expected = match &method.default {
                    None => CallableImplementation::Required,
                    Some(MethodDefault::Script) => CallableImplementation::Script,
                    Some(MethodDefault::Native(HirNativeBinding::Entry(binding))) => {
                        CallableImplementation::Native(binding.clone())
                    }
                    Some(MethodDefault::Native(HirNativeBinding::Default(application))) => {
                        CallableImplementation::NativeDefault(application.clone())
                    }
                    Some(MethodDefault::Native(HirNativeBinding::Host(_))) => {
                        panic!("installed host default")
                    }
                };
                assert_eq!(record.methods[slot].implementation, expected);
                assert_eq!(record.methods[slot].method_policy, method.policy);
            }
            assert!(contracts.insert(id, record.clone()).is_none());
        }
    }
    assert_eq!(contracts.len(), Protocol::ALL.len());
    for (id, record) in &contracts {
        let interface = NominalTy {
            declaration: id.clone(),
            arguments: record
                .generic_params
                .iter()
                .map(|parameter| parameter.as_type())
                .collect(),
            associated_types: BTreeMap::new(),
        };
        assert!(
            trait_closure(
                &interface,
                &Ty::SelfType(id.clone()),
                &Default::default(),
                &|id| contracts.get(id)
            )
            .is_ok(),
            "{}",
            record.name
        );
    }
    let add = traits::identity(Protocol::Add);
    let method = &contracts[&add].methods[0];
    assert_eq!(method.params[0].ty, Ty::SelfType(add.clone()));
    let Ty::Projection {
        receiver,
        interface,
        member,
        arguments,
    } = &method.return_type
    else {
        panic!("source-owned operator output projection");
    };
    assert_eq!(**receiver, Ty::SelfType(add.clone()));
    assert_eq!(interface.declaration, add);
    assert_eq!(*member, associated_type_id(&add, "Output"));
    assert!(arguments.is_empty());
}

#[test]
fn every_installed_callable_and_public_contract_passes_portable_validation() {
    let mut sources = SourceDatabase::default();
    sources
        .set(
            "native-contracts.kgr",
            "fn main() {}".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    for declared in snapshot.declaration_snapshot().files() {
        let analyzed = snapshot.file(declared.source().id()).unwrap();
        let abi = collect_module_abi(analyzed.to_unverified(&Default::default()).unwrap().facts());
        let identity = declared.source().module_identity();
        for declaration in &abi.native_declarations {
            assert!(
                verify::validate_native_declarations(
                    std::slice::from_ref(declaration),
                    identity,
                    &Default::default()
                )
                .is_ok(),
                "invalid native declaration {declaration:#?}"
            );
        }
        for item in &abi.public_items {
            assert!(
                verify::validate(std::slice::from_ref(item), identity, &Default::default()).is_ok(),
                "invalid public declaration {identity}: {item:#?}"
            );
        }
        verify::validate(&abi.public_items, identity, &Default::default()).unwrap();
    }
}

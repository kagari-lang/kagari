use super::collect_module_abi;
use kagari_abi::{
    callable::{CallableImplementation, NativeBinding},
    standard::{
        surface::StandardEnum,
        traits::{self, StandardTrait},
    },
    types::{
        self as abi, AbiType, NominalAbiType, PublicAbiItem, TypeAbiKind,
        inheritance::trait_closure, native::NativeTypeConstructor, verify,
    },
};
use kagari_common::{
    identity::associated_type_id,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_hir::{
    aggregates::MethodDefault, analysis::AnalysisDatabase,
    native::NativeBinding as HirNativeBinding,
};
use std::collections::BTreeMap;

#[test]
fn installed_native_declarations_keep_public_representation_and_payload_contracts() {
    let mut sources = SourceDatabase::default();
    sources
        .set("native-abi.kgr", "fn main() {}".into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let mut count = 0;
    let mut seen_map = false;
    let mut seen_result = false;
    for declared in snapshot.declaration_snapshot().files() {
        if declared.source().module_identity().package.0 != "kagari-std" {
            continue;
        }
        let analyzed = snapshot.file(declared.source().id()).unwrap();
        assert!(
            analyzed.result().diagnostics().is_empty(),
            "{}: {:?}",
            declared.source().name(),
            analyzed.result().diagnostics()
        );
        let abi = collect_module_abi(analyzed.result().facts());
        let native: Vec<_> = abi.public_items.into_iter().filter(|item| {
            matches!(item, PublicAbiItem::Type(ty) if matches!(ty.kind, TypeAbiKind::Native(_)))
        }).collect();
        verify::validate(
            &native,
            declared.source().module_identity(),
            &Default::default(),
        )
        .unwrap();
        for item in &native {
            let PublicAbiItem::Type(ty) = item else {
                unreachable!()
            };
            assert!(ty.fields.is_empty());
            match ty.kind {
                TypeAbiKind::Native(NativeTypeConstructor::Map) => {
                    assert_eq!(ty.name, "LinkedHashMap");
                    assert_eq!(ty.generic_params.len(), 2);
                    seen_map = true;
                }
                TypeAbiKind::Native(NativeTypeConstructor::Enum(StandardEnum::Result)) => {
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
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let root = snapshot.file(root).unwrap();
    let mut contracts = BTreeMap::new();
    for declared in snapshot.declaration_snapshot().files() {
        if declared.source().module_identity().package.0 != "kagari-std" {
            continue;
        }
        let analyzed = snapshot.file(declared.source().id()).unwrap();
        assert!(
            analyzed.result().diagnostics().is_empty(),
            "{}: {:?}",
            declared.source().name(),
            analyzed.result().diagnostics()
        );
        let module = collect_module_abi(analyzed.result().facts());
        for kind in StandardTrait::ALL {
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
            let source = root.result().facts().aggregates.trait_(&id).unwrap();
            assert_eq!(record.generic_params.len(), source.generic_params.len());
            assert_eq!(record.methods.len(), source.methods.len());
            for (slot, method) in source.methods.iter().enumerate() {
                assert_eq!(record.methods[slot].name, method.name);
                let expected = match method.default {
                    None => CallableImplementation::Required,
                    Some(MethodDefault::Script) => CallableImplementation::Script,
                    Some(MethodDefault::Native(HirNativeBinding::Engine(binding))) => {
                        CallableImplementation::Native(NativeBinding::Engine(binding))
                    }
                    Some(MethodDefault::Native(_)) => panic!("installed host default"),
                };
                assert_eq!(record.methods[slot].implementation, expected);
                assert_eq!(record.methods[slot].method_policy, method.policy);
            }
            assert!(contracts.insert(id, record.clone()).is_none());
        }
    }
    assert_eq!(contracts.len(), StandardTrait::ALL.len());
    for (id, record) in &contracts {
        let interface = NominalAbiType {
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
                &AbiType::SelfType(id.clone()),
                &Default::default(),
                &|id| contracts.get(id)
            )
            .is_ok(),
            "{}",
            record.name
        );
    }
    let add = traits::identity(StandardTrait::Add);
    let method = &contracts[&add].methods[0];
    assert_eq!(method.params[0].ty, AbiType::SelfType(add.clone()));
    let AbiType::Projection {
        receiver,
        interface,
        member,
        arguments,
    } = &method.return_type
    else {
        panic!("source-owned operator output projection");
    };
    assert_eq!(**receiver, AbiType::SelfType(add.clone()));
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
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    for declared in snapshot.declaration_snapshot().files() {
        let analyzed = snapshot.file(declared.source().id()).unwrap();
        let abi = collect_module_abi(analyzed.result().facts());
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

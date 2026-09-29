use super::collect_module_abi;
use kagari_abi::{
    standard::surface::StandardEnum,
    types::{PublicAbiItem, TypeAbiKind, native::NativeTypeConstructor, verify},
};
use kagari_common::source_database::{SourceDatabase, SourceLayer};
use kagari_hir::analysis::AnalysisDatabase;

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

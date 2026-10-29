use super::program_for_source;
use kagari_derive::KagariValue;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    gc::GcHeapConfig,
    native::{
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{KagariType, context::ConversionContext},
        module::NativeModule,
        payload::Opaque,
        storage::{
            NativePayload, NativeStorage,
            scan::{NativeScanKind, ScanBudget, ScanStatus},
            value::StoredValue,
        },
        typed::NativeContext,
        types::Type,
    },
    resource::HeapCharge,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::ty::Ty;

kagari_runtime::native_data! { struct GameConfig { power: i32 } }

#[derive(KagariValue)]
#[kagari(name = "Input")]
struct BattleInput {
    label: String,
    config: Opaque<GameConfig>,
}

#[test]
fn derived_mixed_dto_keeps_opaque_identity_and_checked_data_capabilities() {
    let providers = StandardDeclarations::default().catalog().unwrap();
    let mut builder = ModuleBuilder::new("game::mixed", &providers).unwrap();
    builder.register_type::<BattleInput>().unwrap();
    assert!(builder.register_type::<Opaque<GameConfig>>().is_err());
    let module = builder.finish().unwrap();
    assert_eq!(module.declaration().types.len(), 2);
    assert!(module.declaration().functions.is_empty());
    assert!(module.declaration().implementations.is_empty());
    let mapping = Opaque::<GameConfig>::kagari_type(&module.catalog()).unwrap();
    let program = program_for_source(
        &module,
        "use game::mixed::Input; pub fn inspect(input: Input) -> String { input.label }",
    );
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        ..Default::default()
    });
    let mut modules = kagari_stdlib::modules().unwrap();
    modules.push(module);
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let owner_program = runtime.load_program("mixed", program).unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();
    let Ty::NativeObject(nominal) = mapping.abi() else {
        panic!("opaque mapping")
    };
    let descriptor = runtime
        .bind_native_type_declaration::<GameConfig>(
            &owner_program.root(),
            nominal.declaration,
            mapping.definitions().unwrap(),
            &[],
        )
        .unwrap();
    let native = NativeContext::new(&runtime, &owner).unwrap();
    let config = descriptor
        .create(
            &mut NativeContext::new(&runtime, &owner).unwrap(),
            GameConfig { power: 7 },
            &[],
        )
        .unwrap();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let root = cx
        .encode(BattleInput {
            label: "initial".into(),
            config: config.clone(),
        })
        .unwrap();
    let mut decoded: BattleInput = cx.decode(&root).unwrap();
    decoded.label = "Rust snapshot".into();
    decoded
        .config
        .edit_data(&native, |config| {
            config.power = 12;
            Ok(())
        })
        .unwrap();
    runtime.collect_garbage().unwrap();
    let again: BattleInput = cx.decode(&root).unwrap();
    assert_eq!(again.label, "initial");
    assert_eq!(
        again
            .config
            .read_data(&native, |config| Ok(config.power))
            .unwrap(),
        12
    );
    #[cfg(feature = "execution-diagnostics")]
    {
        let before = runtime.gc().storage_usage().unwrap();
        assert!(before.native_payload_bytes >= size_of::<GameConfig>());
        assert!(
            config
                .read_data(&native, |_| runtime.gc().storage_usage())
                .is_err()
        );
        assert_eq!(runtime.gc().storage_usage().unwrap(), before);
        assert_eq!(
            config
                .read_data(&native, |config| Ok(config.power))
                .unwrap(),
            12
        );
    }
    assert_eq!(config.take(&native).unwrap().power, 12);
    assert!(decoded.config.read_data(&native, |_| Ok(())).is_err());
    assert!(again.config.read_data(&native, |_| Ok(())).is_err());
    assert!(cx.decode::<BattleInput>(&root).is_err());
    runtime.collect_garbage().unwrap();
    assert!(!runtime.is_quarantined());
}

#[derive(Debug)]
struct InvalidStorage;

unsafe impl NativePayload for InvalidStorage {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan<'payload>(
        &'payload self,
        _: &mut Self::ScanState,
        budget: &mut ScanBudget,
        _: &mut dyn FnMut(&'payload StoredValue),
    ) -> ScanStatus {
        budget.charge(1);
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(0, 0)
    }

    fn registration_storage() -> NativeStorage {
        NativeStorage::data::<GameConfig>()
    }
}

#[test]
fn automatic_opaque_storage_identity_rejects_without_publishing_a_mapping() {
    let mut module = ModuleBuilder::new("game::invalid", &DeclarationCatalog::default()).unwrap();
    assert!(module.register_type::<Opaque<InvalidStorage>>().is_err());
    assert!(
        module
            .declarations()
            .rust_type::<Opaque<InvalidStorage>>()
            .is_err()
    );
    let good: Type = module.register_type::<Opaque<GameConfig>>().unwrap();
    assert!(matches!(good.abi(), Ty::NativeObject(_)));
    assert_eq!(module.finish().unwrap().declaration().types.len(), 1);
}

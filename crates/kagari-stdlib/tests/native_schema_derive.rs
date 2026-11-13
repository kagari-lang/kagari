use super::program_for_source;
use crate::bytecode_fixture::load_bytecode;
use kagari_derive::KagariValue;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    execution::memory::GcHeapConfig,
    native::{
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{KagariType, context::ConversionContext},
        module::NativeModule,
        payload::Opaque,
        storage::{NativePayload, NativeStorage},
        typed::NativeContext,
        types::Type,
    },
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
    let program = program_for_source(&module, include_str!("fixtures/native/schema_mixed.kgr"));
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
    let owner_program = load_bytecode(&mut runtime, "mixed", program).unwrap();
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
    let config = descriptor
        .create(
            &mut NativeContext::new(&mut runtime, &owner).unwrap(),
            GameConfig { power: 7 },
        )
        .unwrap();
    let (root, mut decoded) = {
        let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
        let root = cx
            .encode(BattleInput {
                label: "initial".into(),
                config: config.clone(),
            })
            .unwrap();
        let decoded: BattleInput = cx.decode(&root).unwrap();
        (root, decoded)
    };
    let mut native = NativeContext::new(&mut runtime, &owner).unwrap();
    decoded.label = "Rust snapshot".into();
    decoded
        .config
        .edit(&mut native, |config| {
            config.power = 12;
            Ok(())
        })
        .unwrap();
    native.collect_garbage().unwrap();
    let again: BattleInput = ConversionContext::new(native.runtime_mut(), &owner)
        .unwrap()
        .decode(&root)
        .unwrap();
    assert_eq!(again.label, "initial");
    assert_eq!(
        again
            .config
            .read(&mut native, |config| Ok(config.power))
            .unwrap(),
        12
    );
    native.collect_garbage().unwrap();
    assert!(!native.runtime().is_quarantined());
}

#[derive(Debug)]
struct InvalidStorage;

impl NativePayload for InvalidStorage {
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

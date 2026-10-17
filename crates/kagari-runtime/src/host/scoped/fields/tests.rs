use crate::{
    Runtime,
    native::{
        builder::ModuleBuilder, catalog::DeclarationCatalog, conversion::context::ConversionContext,
    },
    value::Value,
};
use kagari_bytecode::{
    instruction::StructId,
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{DefinitionKind, metadata::DefinitionMetadata};
use kagari_contract::layout::{StructFieldLayout, StructLayout};
use kagari_contract::types::PublicItem;
use kagari_derive::KagariValue;
use kagari_types::{reference::ReferenceAccess, scalar::BuiltinType, ty::Ty};

#[derive(KagariValue)]
struct Stats {
    hp: i32,
}

#[derive(KagariValue)]
struct Player {
    stats: Stats,
    label: String,
}

#[test]
fn executable_inline_field_codecs_keep_paths_current_and_results_rooted() {
    let mut builder =
        ModuleBuilder::new("test::field_codecs", &DeclarationCatalog::default()).unwrap();
    builder.register_type::<Player>().unwrap();
    let module = builder.finish().unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let catalog = &runtime.native_entries.catalog;
    let public_items = catalog
        .types
        .iter()
        .map(|(_, ty)| PublicItem::Type(ty.clone()))
        .collect();
    let structures: Vec<StructLayout> = [
        catalog.rust_type::<Player>().unwrap(),
        catalog.rust_type::<Stats>().unwrap(),
    ]
    .into_iter()
    .map(|ty| {
        let Ty::Struct(nominal) = ty.abi() else {
            panic!("struct")
        };
        let definition = catalog.types.get_id(nominal.declaration).unwrap();
        StructLayout {
            declaration: nominal.declaration,
            arguments: vec![],
            fields: definition
                .fields
                .iter()
                .map(|field| StructFieldLayout {
                    declaration: runtime
                        .definition_context()
                        .snapshot()
                        .lookup_child(nominal.declaration, DefinitionKind::Field, &field.name, 0)
                        .unwrap(),
                    visibility: field.visibility,
                    name: field.name.clone(),
                    ty: field.ty.clone(),
                    mutable: field.mutable,
                })
                .collect(),
        }
    })
    .collect();
    let identity = runtime
        .definition_context()
        .snapshot()
        .resolve(structures[0].declaration)
        .unwrap()
        .module()
        .clone();
    let program = DefinitionMetadata::checked(
        runtime.definition_context().snapshot(),
        BytecodeProgram {
            root: ModuleRef::new(0),
            exports: Default::default(),
            modules: vec![BytecodeModule {
                structures,
                public_items,
                identity,
                ..Default::default()
            }],
        },
        &Default::default(),
    )
    .unwrap();
    let owner = runtime
        .load_program("field-codec-proof", program.clone())
        .unwrap();
    let player_layout = owner.struct_layout(StructId::new(0)).unwrap();
    let stats_layout = owner.struct_layout(StructId::new(1)).unwrap();
    let player_type = runtime.bind_rust_type::<Player>(&owner).unwrap();
    let mut changed = program.clone().into_records();
    changed.modules[0].structures[1].fields[0].ty = Ty::Builtin(BuiltinType::I64);
    for item in &mut changed.modules[0].public_items {
        if let PublicItem::Type(ty) = item
            && ty.name == "Stats"
        {
            ty.fields[0].ty = Ty::Builtin(BuiltinType::I64);
        }
    }
    let changed = DefinitionMetadata::checked(
        runtime.definition_context().snapshot(),
        changed,
        &Default::default(),
    )
    .unwrap();
    assert!(runtime.load_program("field-codec-proof", changed).is_err());
    let newer = runtime.load_program("field-codec-proof", program).unwrap();
    let newer_layout = newer.struct_layout(StructId::new(0)).unwrap();
    let mut player = Player {
        stats: Stats { hp: 40 },
        label: "original".into(),
    };
    runtime
        .with_host_scope(|scope| {
            let whole = scope.borrow_mut(&player_type, &mut player)?;
            scope.execute(&[whole.value()], |runtime, values| {
                let view = runtime.scoped_view(&values[0])?;
                let compatible = runtime.host_borrows.read_field(
                    runtime,
                    view,
                    &newer_layout,
                    0,
                    ReferenceAccess::Shared,
                )?;
                assert!(compatible.get(runtime.gc(), 0).is_some());
                let stats = runtime.host_borrows.read_field(
                    runtime,
                    view,
                    &player_layout,
                    0,
                    ReferenceAccess::Mutable,
                )?;
                let stats_value = stats.get(runtime.gc(), 0).unwrap();
                let stats_view = runtime.scoped_view(&stats_value)?;
                runtime.host_borrows.write_field(
                    runtime,
                    stats_view,
                    &stats_layout,
                    0,
                    Value::I32(70),
                )?;
                assert_eq!(
                    runtime
                        .host_borrows
                        .read_field(
                            runtime,
                            stats_view,
                            &stats_layout,
                            0,
                            ReferenceAccess::Mutable
                        )?
                        .get(runtime.gc(), 0),
                    Some(Value::I32(70))
                );
                let shared = runtime.host_borrows.read_field(
                    runtime,
                    view,
                    &player_layout,
                    0,
                    ReferenceAccess::Shared,
                )?;
                let shared = runtime.scoped_view(&shared.get(runtime.gc(), 0).unwrap())?;
                assert!(
                    runtime
                        .host_borrows
                        .write_field(runtime, shared, &stats_layout, 0, Value::I32(99))
                        .is_err()
                );
                runtime
                    .host_borrows
                    .read_scoped::<Stats, _>(runtime, stats_view, |_| {
                        assert!(
                            runtime
                                .host_borrows
                                .write_field(runtime, stats_view, &stats_layout, 0, Value::I32(99))
                                .is_err()
                        );
                        Ok(())
                    })?;
                let mut cx = ConversionContext::new(runtime, &owner)?;
                let replacement = cx.encode(Stats { hp: 5 })?;
                runtime.host_borrows.write_field(
                    runtime,
                    view,
                    &player_layout,
                    0,
                    replacement.value(runtime.gc()).unwrap(),
                )?;
                assert_eq!(
                    runtime
                        .host_borrows
                        .read_field(
                            runtime,
                            stats_view,
                            &stats_layout,
                            0,
                            ReferenceAccess::Mutable
                        )?
                        .get(runtime.gc(), 0),
                    Some(Value::I32(5))
                );
                let label = runtime.host_borrows.read_field(
                    runtime,
                    view,
                    &player_layout,
                    1,
                    ReferenceAccess::Shared,
                )?;
                runtime.collect_garbage()?;
                let Value::Str(id) = label.get(runtime.gc(), 0).unwrap() else {
                    panic!("string")
                };
                assert_eq!(&*runtime.gc().string(id).unwrap(), "original");
                let changed = cx.encode("changed".to_string())?;
                runtime
                    .host_borrows
                    .read_scoped::<Stats, _>(runtime, stats_view, |stats| {
                        assert_eq!(stats.hp, 5);
                        runtime.host_borrows.write_field(
                            runtime,
                            view,
                            &player_layout,
                            1,
                            changed.value(runtime.gc()).unwrap(),
                        )?;
                        assert_eq!(stats.hp, 5);
                        Ok(())
                    })?;
                Ok(Value::Unit)
            })?;
            Ok(())
        })
        .unwrap();
    assert_eq!(player.stats.hp, 5);
    assert_eq!(player.label, "changed");
}

#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime,
    execution::roots::{ExecutionRoots, SessionId},
    host::scoped::ScopedHostView,
    module::StructLayoutRef,
    native::binding::NativeResult,
    native::{
        builder::ModuleBuilder, catalog::DeclarationCatalog, conversion::context::ConversionContext,
    },
    value::{carriers::ScopedValue, place::ResultPlace, view::ValueView},
};
use kagari_bytecode::{instruction::StructId, module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::{DefinitionKind, metadata::DefinitionMetadata};
use kagari_contract::layout::{StructFieldLayout, StructLayout};
use kagari_contract::types::PublicItem;
use kagari_derive::KagariValue;
use kagari_types::{reference::ReferenceAccess, scalar::BuiltinType, ty::Ty};
use std::any::TypeId;

#[derive(KagariValue)]
struct Stats {
    hp: i32,
}

#[derive(KagariValue)]
struct Player {
    stats: Stats,
    label: String,
}

// This test owns the raw result execution until every field result is consumed.
// Each read publishes before its conversion/Host loan retires, as a frame does.
fn read<'source>(
    runtime: &mut Runtime,
    execution: SessionId,
    view: ScopedHostView,
    layout: &StructLayoutRef,
    slot: usize,
    access: ReferenceAccess,
) -> NativeResult<ValueView<'source>> {
    let destination = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                slots.push_local(execution, ScopedValue::unit())
            })
        })
        .map_err(|error| runtime.managed_memory_error(error))?;
    runtime
        .read_scoped_field(view, layout, slot, access, ResultPlace::new(destination))?
        .admit(destination)?;
    runtime
        .memory
        .inspect_value(destination)
        .map_err(|error| runtime.managed_memory_error(error))
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
    let program = ProgramFixture {
        root: ModuleRef::new(0),
        exports: Default::default(),
        modules: vec![BytecodeModule {
            structures,
            public_items,
            identity,
            ..Default::default()
        }],
    }
    .metadata(runtime.definition_context().snapshot(), &Default::default())
    .unwrap();
    let owner_program = load_bytecode(&mut runtime, "field-codec-proof", program.clone()).unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();
    let player_layout = owner.struct_layout(StructId::new(0)).unwrap();
    let stats_layout = owner.struct_layout(StructId::new(1)).unwrap();
    let player_type = runtime
        .bind_rust_type::<Player>(&owner_program.root())
        .unwrap();
    let mut changed = program.clone().into_records();
    changed.modules[0].structures[1].fields[0].ty = Ty::Builtin(BuiltinType::I64);
    for item in &mut changed.modules[0].public_items {
        if let PublicItem::Type(ty) = item
            && ty.name == "Stats"
        {
            ty.fields[0].ty = Ty::Builtin(BuiltinType::I64);
        }
    }
    let changed =
        DefinitionMetadata::checked(program.definitions().clone(), changed, &Default::default())
            .unwrap();
    assert!(load_bytecode(&mut runtime, "field-codec-proof", changed).is_err());
    let staged = runtime
        .stage_reload_program(&owner_program, None, program)
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
    let newer = runtime.select_program(&owner_program).unwrap();
    let newer_layout = newer.struct_layout(StructId::new(0)).unwrap();
    let mut player = Player {
        stats: Stats { hp: 40 },
        label: "original".into(),
    };
    let results = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let roots_before = runtime.gc_stats().root_registrations;
    runtime
        .with_host_scope(|scope| {
            let whole = scope.borrow_mut(&player_type, &mut player)?;
            scope.execute(&[whole.value()], |runtime, values, destination| {
                let value = runtime
                    .memory
                    .scope(&runtime.resources, |cx, roots| {
                        ExecutionRoots::with(roots, cx, |slots| values.view(slots, 0))
                    })
                    .map_err(|error| runtime.managed_memory_error(error))?;
                let view = runtime.scoped_view(&value)?;
                let compatible = read(
                    runtime,
                    results,
                    view,
                    &newer_layout,
                    0,
                    ReferenceAccess::Shared,
                )?;
                assert!(runtime.scoped_view(&compatible).is_ok());
                let stats = read(
                    runtime,
                    results,
                    view,
                    &player_layout,
                    0,
                    ReferenceAccess::Mutable,
                )?;
                let stats_view = runtime.scoped_view(&stats)?;
                runtime.write_scoped_field(
                    stats_view,
                    &stats_layout,
                    0,
                    &ValueView::from(70i32),
                )?;
                assert_eq!(
                    read(
                        runtime,
                        results,
                        stats_view,
                        &stats_layout,
                        0,
                        ReferenceAccess::Mutable
                    )?
                    .as_i32(),
                    Some(70)
                );
                let shared = read(
                    runtime,
                    results,
                    view,
                    &player_layout,
                    0,
                    ReferenceAccess::Shared,
                )?;
                let shared = runtime.scoped_view(&shared)?;
                assert!(
                    runtime
                        .write_scoped_field(shared, &stats_layout, 0, &ValueView::from(99i32))
                        .is_err()
                );
                runtime.with_host_loans(
                    [(stats_view, TypeId::of::<Stats>(), ReferenceAccess::Shared)],
                    |runtime, _| {
                        assert!(
                            runtime
                                .write_scoped_field(
                                    stats_view,
                                    &stats_layout,
                                    0,
                                    &ValueView::from(99i32)
                                )
                                .is_err()
                        );
                        Ok(())
                    },
                )?;
                assert_eq!(runtime.gc_stats().root_registrations, roots_before);
                let mut cx = ConversionContext::new(runtime, &owner)?;
                let stats_type = cx.type_for::<Stats>()?;
                let replacement = cx.encode_value(&stats_type, Stats { hp: 5 })?;
                cx.runtime_mut()
                    .write_scoped_field(view, &player_layout, 0, &replacement)?;
                assert_eq!(
                    read(
                        cx.runtime_mut(),
                        results,
                        stats_view,
                        &stats_layout,
                        0,
                        ReferenceAccess::Mutable
                    )?
                    .as_i32(),
                    Some(5)
                );
                let label = read(
                    cx.runtime_mut(),
                    results,
                    view,
                    &player_layout,
                    1,
                    ReferenceAccess::Shared,
                )?;
                cx.runtime_mut().collect_garbage()?;
                assert_eq!(cx.read_string(&label, str::to_owned)?, "original");
                let string_type = cx.type_for::<String>()?;
                let changed = cx.encode_value(&string_type, "changed".to_string())?;
                cx.runtime_mut().with_host_loans(
                    [(stats_view, TypeId::of::<Stats>(), ReferenceAccess::Shared)],
                    |runtime, [stats]| {
                        // SAFETY: The exact shared Stats loan remains active. A
                        // disjoint generated field write cannot invalidate it.
                        let stats = unsafe { &*stats.cast::<Stats>() };
                        assert_eq!(stats.hp, 5);
                        runtime.write_scoped_field(view, &player_layout, 1, &changed)?;
                        assert_eq!(stats.hp, 5);
                        Ok(())
                    },
                )?;
                assert_eq!(cx.runtime().gc_stats().root_registrations, roots_before);
                drop(cx);
                destination.publish(runtime, &ValueView::unit())
            })?;
            Ok(())
        })
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, results)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots_before);
    assert_eq!(player.stats.hp, 5);
    assert_eq!(player.label, "changed");
}

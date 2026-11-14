//! An ordinary DTO schema permits explicit external loans, never GC-to-Rust casts.
use super::source_free_vm;
use kagari_runtime::{
    Runtime,
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, declarations::MethodDecl,
        module::NativeModule, typed::loans::LoanContext, types::Type,
    },
    value::view::ValueView,
};

use crate::declarations::StandardDeclarations;
use kagari_derive::KagariValue;
use kagari_hir::analyze_source;
use kagari_source::source::SourceFile;
use kagari_types::{reference::ReferenceAccess, scalar::BuiltinType};
use std::sync::Arc;

#[derive(KagariValue)]
struct Player {
    hp: i32,
    label: String,
}

fn registration() -> NativeModule {
    let providers = StandardDeclarations::default().catalog().unwrap();
    let mut module = ModuleBuilder::new("example::struct_scope", &providers).unwrap();
    let player = module.register_type::<Player>().unwrap();
    module
        .implement(player, |implementation| {
            implementation.inherent_impl(|methods| {
                let inspect = methods.define_method(
                    MethodDecl::instance("inspect")
                        .receiver_access(ReferenceAccess::Shared)
                        .returns(Type::scalar(BuiltinType::String)),
                )?;
                methods.bind(
                    inspect,
                    |cx: &mut LoanContext<'_>, (player,): (&Player,)| {
                        cx.poll()?;
                        Ok(format!("{} hp={}", player.label, player.hp))
                    },
                )?;
                let damage = methods.define_method(
                    MethodDecl::instance("damage")
                        .receiver_access(ReferenceAccess::Mutable)
                        .parameter("amount", Type::i32())
                        .returns(Type::i32()),
                )?;
                methods.bind(
                    damage,
                    |cx: &mut LoanContext<'_>, (player, amount): (&mut Player, i32)| {
                        cx.poll()?;
                        player.hp -= amount;
                        Ok(player.hp)
                    },
                )?;
                Ok(())
            })
        })
        .unwrap();
    module.finish().unwrap()
}

const SOURCE: &str =
    include_str!("../../../../../tests/fixtures/native/conversion-scoped-struct.kgr");

#[test]
fn registered_struct_loans_use_original_host_storage_and_common_methods() {
    let module = registration();
    assert_eq!(module.declaration().types.len(), 1);
    let (mut vm, owner) = source_free_vm(SOURCE, &module);
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let descriptor = vm.bind_rust_type::<Player>(&_owner_program.root()).unwrap();
    assert!(vm.bind_rust_type::<i32>(&_owner_program.root()).is_err());
    let mut external = Player {
        hp: 10,
        label: "external".into(),
    };
    let original = ConversionContext::new(&mut vm, &owner)
        .unwrap()
        .encode(Player {
            hp: external.hp,
            label: external.label.clone(),
        })
        .unwrap();
    vm.with_host_scope(|scope| {
        let view = scope.borrow_mut(&descriptor, &mut external)?;
        let text = Runtime::execute_scoped(
            scope,
            &_owner_program,
            "inspect",
            &[view.value()],
            &Default::default(),
        )?;
        assert_eq!(
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "damage",
                &[view.value(), ValueView::from(3i32)],
                &Default::default()
            )?
            .as_i32(),
            Some(7)
        );
        // A later invocation may collect; the earlier heap result is retained
        // for the active scope, independently of the original Rust object.
        scope.execute(&[text], |runtime, values, result| {
            runtime.collect_garbage()?;
            let mut cx = ConversionContext::new(runtime, &owner)?;
            let text = values.get(&mut cx, 0)?;
            cx.read_string(&text, |text| assert_eq!(text, "external hp=10"))?;
            result.publish(cx.runtime_mut(), &ValueView::unit())
        })?;
        view.read(scope, |_scope, player| {
            assert_eq!(player.hp, 7);
            Ok(())
        })?;
        assert!(
            view.read(scope, |scope, _| Runtime::execute_scoped(
                scope,
                &_owner_program,
                "damage",
                &[view.value(), ValueView::from(1i32)],
                &Default::default()
            )
            .map(|_| ()))
                .is_err()
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(external.hp, 7);
    let mut cx = ConversionContext::new(&mut vm, &owner).unwrap();
    assert_eq!(cx.decode::<Player>(&original).unwrap().hp, 10);
    let copied: Player = cx
        .runtime_mut()
        .execute_typed(
            &_owner_program,
            "copy",
            (Player {
                hp: 2,
                label: "DTO".into(),
            },),
            &Default::default(),
        )
        .unwrap();
    assert_eq!(copied.hp, 2);
}

#[test]
fn ordinary_gc_structs_and_shared_views_cannot_call_mutable_rust_loans() {
    let module = registration();
    let mut providers = crate::catalog::shared().unwrap();
    providers.push(Arc::new(module.to_declaration()));
    for source in [
        "fn bad(player: Player) -> String { player.inspect() }",
        "fn bad(player: Player) -> i32 { player.damage(1) }",
        "fn bad(player: Ref<Player>) -> i32 { player.damage(1) }",
    ] {
        let analysis = analyze_source(
            &SourceFile::new(
                "reject.kgr",
                format!("use example::struct_scope::Player; {source}"),
            ),
            providers.clone(),
        )
        .unwrap();
        assert!(!analysis.diagnostics().is_empty(), "{source}");
    }
}

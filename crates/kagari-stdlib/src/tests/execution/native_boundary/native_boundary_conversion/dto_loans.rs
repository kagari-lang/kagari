use crate::test_library;
use kagari_derive::KagariValue;
use kagari_hir::analyze_source;
use kagari_runtime::native::{
    builder::ModuleBuilder, declarations::MethodDecl, typed::loans::LoanContext, types::Type,
};
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_types::reference::ReferenceAccess;
use std::sync::Arc;

#[derive(KagariValue)]
struct Player {
    hp: i32,
}

#[test]
fn ordinary_gc_structs_cannot_call_rust_receiver_loans() {
    let catalog = test_library::standard().catalog();
    let mut module = ModuleBuilder::new("example::dto_loans", catalog).unwrap();
    let player = module.register_type::<Player>().unwrap();
    module
        .implement(player, |implementation| {
            implementation.inherent_impl(|methods| {
                let inspect = methods.define_method(
                    MethodDecl::instance("inspect")
                        .receiver_access(ReferenceAccess::Shared)
                        .returns(Type::i32()),
                )?;
                methods.bind(inspect, |_: &mut LoanContext<'_>, (player,): (&Player,)| {
                    Ok(player.hp)
                })?;
                let damage = methods.define_method(
                    MethodDecl::instance("damage")
                        .receiver_access(ReferenceAccess::Mutable)
                        .returns(Type::i32()),
                )?;
                methods.bind(
                    damage,
                    |_: &mut LoanContext<'_>, (player,): (&mut Player,)| {
                        player.hp -= 1;
                        Ok(player.hp)
                    },
                )
            })
        })
        .unwrap();
    let mut providers = test_library::standard().declarations().to_vec();
    providers.push(Arc::new(module.finish().unwrap().to_declaration()));
    for member in ["inspect", "damage"] {
        let source = format!(
            "use example::dto_loans::Player; fn reject(player: Player) -> i32 {{ player.{member}() }}"
        );
        let analysis =
            analyze_source(&SourceFile::new("reject.kgr", source), providers.clone()).unwrap();
        assert!(
            analysis.diagnostics().iter().any(|diagnostic| matches!(
                &diagnostic.kind, DiagnosticKind::UnknownName { name } if name == member
            )),
            "{member}: {:?}",
            analysis.diagnostics()
        );
    }
}

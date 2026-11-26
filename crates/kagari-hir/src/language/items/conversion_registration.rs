//! A custom conversion designation is not authority over impls or call resolution.
use super::projection::simple_trait;
use crate::{analysis::AnalysisDatabase, typeck::table::CallTarget, types::TypeId};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, metadata::DefinitionMetadata,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    declaration::{conversion::ConversionAdapter, module::ModuleDecl},
    language::binding,
    scalar::BuiltinType,
};
use std::sync::Arc;

#[test]
fn custom_conversion_designation_does_not_change_sibling_impl_or_selected_result() {
    let module = |name: &str| ModuleIdentity {
        package: PackageId("game".into()),
        path: vec![name.into()],
    };
    let api = module("api");
    let cancel = Default::default();
    let mut sources = SourceDatabase::default();
    sources.bind_module("api.kgr", api.clone()).unwrap();
    let file = sources
        .set(
            "api.kgr",
            include_str!("../../../tests/fixtures/native/convert_trait.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let snapshot = db.snapshot(sources.snapshot(), &cancel).unwrap();
    let file = snapshot.file(file).unwrap();
    let defs = file.definitions();
    let signature = file
        .result()
        .facts()
        .aggregates
        .traits()
        .find(|tr| tr.declaration.name == "Convert")
        .unwrap();
    let mut contract = simple_trait(signature, defs).unwrap();
    assert!(contract.conversion_adapter.is_none());
    let result = binding::result_declaration();
    contract.conversion_adapter = Some(ConversionAdapter {
        method: defs
            .lookup_child(signature.id, DefinitionKind::Method, "convert", 0)
            .unwrap(),
        error: defs
            .lookup_child(signature.id, DefinitionKind::AssociatedType, "Error", 0)
            .unwrap(),
        result: defs.lookup(&result).unwrap(),
    });
    let mut registration = ModuleDecl::new(api.clone());
    registration.dependencies.insert(result.module);
    registration.traits.push(contract);
    let mut direct = AnalysisDatabase::default();
    direct.set_native_modules(vec![Arc::new(
        DefinitionMetadata::checked(defs.clone(), registration, &cancel).unwrap(),
    )]);
    let mut registered = SourceDatabase::default();
    for (db, sources) in [(&mut db, &mut sources), (&mut direct, &mut registered)] {
        sources.bind_module("compat.kgr", module("compat")).unwrap();
        let root = sources
            .set(
                "compat.kgr",
                include_str!("../../../tests/fixtures/native/convert_sibling.kgr").into(),
                SourceLayer::Base,
            )
            .unwrap();
        let snapshot = db.snapshot(sources.snapshot(), &cancel).unwrap();
        let file = snapshot.file(root).unwrap();
        let diagnostics = file.result().diagnostics();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        snapshot.check_program(root, &cancel).unwrap();
        let facts = file.result().facts();
        let defs = file.definitions();
        let convert = defs
            .lookup_child(
                defs.lookup_root(&api).unwrap(),
                DefinitionKind::Trait,
                "Convert",
                0,
            )
            .unwrap();
        let member = defs
            .lookup_child(convert, DefinitionKind::Method, "convert", 0)
            .unwrap();
        let hir = &facts.lowered.module;
        let caller = hir
            .functions
            .iter()
            .find(|f| f.name == "convert_flag")
            .unwrap();
        let call = hir
            .block(caller.implementation.script_body().unwrap())
            .tail_expr
            .unwrap();
        let types = facts.typed.type_table();
        let target = &types.call_resolution(call).unwrap().target;
        assert!(
            matches!(target, CallTarget::TraitMethod { method, interface } if *method == member && interface.arguments == [TypeId::Builtin(BuiltinType::Bool)])
        );
        assert!(
            matches!(types.expr_type(call), Some(TypeId::Enum(output)) if Some(output.declaration) == defs.lookup(&binding::result_declaration()) && output.arguments == [TypeId::Builtin(BuiltinType::I32), TypeId::Builtin(BuiltinType::I64)])
        );
    }
}

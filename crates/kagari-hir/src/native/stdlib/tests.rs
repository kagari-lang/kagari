use super::*;
use crate::{
    declarations::DeclarationId,
    declare_analysis,
    hir::{ExportItem, Item, Visibility},
    host::HostDeclarations,
    imports::ModuleGraph,
    resolver::ResolvedName,
    types::TypeId,
};
use kagari_abi::scalar::BuiltinType;
use kagari_common::{SourceFile, collection::CollectionAccess};
use kagari_syntax::parser;

#[test]
fn installed_opaque_types_enter_the_ordinary_declaration_graph() {
    let cancel = Default::default();
    let installed = InstalledStdlib::prepare(Default::default(), &cancel).unwrap();
    let hosts = HostDeclarations::empty();
    let graph =
        ModuleGraph::build(installed.modules.iter().map(AsRef::as_ref), &hosts, &cancel).unwrap();
    let module = installed
        .modules
        .iter()
        .find(|module| module.source.name() == "kagari://std/array.kgr")
        .unwrap();
    let opaque = &module.module.opaque_types[0];
    assert_eq!(opaque.name, "ArrayList");
    assert_eq!(opaque.visibility, Visibility::Public);
    assert!(
        module
            .module
            .exports
            .iter()
            .any(|item| item.item == ExportItem::OpaqueType(opaque.id))
    );
    assert!(module.module.items.contains(&Item::OpaqueType(opaque.id)));
    assert!(Arc::ptr_eq(
        module.installed_stdlib.as_ref().unwrap(),
        &installed.package
    ));
    let imports = graph
        .node(module.source.module_identity())
        .unwrap()
        .imports
        .clone();
    let declared = declare_analysis(module.clone(), hosts, imports, &cancel);
    assert!(
        declared.names.diagnostics.is_empty(),
        "{:?}",
        declared.names.diagnostics
    );
    let target = declared
        .names
        .facts
        .items
        .lookup("ArrayList")
        .unwrap()
        .target()
        .unwrap();
    assert_eq!(target, ResolvedName::OpaqueType(opaque.id));
    let declaration = declared.declarations.target(target).unwrap();
    assert_eq!(
        &module.source.text()[declaration.location.range.start..declaration.location.range.end],
        "ArrayList"
    );
    let DeclarationId::Definition(id) = &declaration.id else {
        panic!("ordinary definition");
    };
    let parameters = declared.declarations.parameters_of(id);
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].owner, *id);
    let kind = declared.declarations.native_type(opaque.id).unwrap();
    assert_eq!(
        kind.apply(&[TypeId::Builtin(BuiltinType::I32)]),
        Some(TypeId::Array(
            Box::new(TypeId::Builtin(BuiltinType::I32)),
            CollectionAccess::Mutable
        ))
    );
    assert_eq!(kind.apply(&[]), None);
}

#[test]
fn matching_uri_and_native_attribute_do_not_install_a_storage_binding() {
    let source = Arc::new(SourceFile::new(
        "kagari://std/array.kgr",
        "#[builtin_type(ArrayList)] pub type ArrayList<T>;",
    ));
    let cancel = Default::default();
    let parsed = parser::parse_declarations(&source, Default::default(), &cancel).unwrap();
    assert!(parsed.diagnostics().is_empty());
    let lowered = lower_module_controlled(source, &parsed.syntax(), &cancel);
    assert_eq!(lowered.module.opaque_types.len(), 1);
    assert!(lowered.installed_stdlib.is_none());
    assert!(lowered.native_types.is_empty());
}

#[test]
fn opaque_type_lowering_preserves_bounds_and_alias_syntax_for_validation() {
    let source = Arc::new(SourceFile::new(
        "types.kgr",
        "pub type Sequence<T: Hash>: Debug = ArrayList<T> where T: Eq;",
    ));
    let cancel = Default::default();
    let parsed = parser::parse_declarations(&source, Default::default(), &cancel).unwrap();
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    let lowered = lower_module_controlled(source, &parsed.syntax(), &cancel);
    let declaration = &lowered.module.opaque_types[0];
    assert_eq!(declaration.generic_params[0].bounds.len(), 1);
    assert_eq!(declaration.trait_bounds.len(), 1);
    assert_eq!(declaration.bounds.len(), 1);
    assert!(declaration.definition.is_some());
    assert!(lowered.native_types.is_empty());
}

#[test]
fn native_storage_arity_is_validated_before_installation() {
    let cancel = Default::default();
    let package = ParsedStdlibPackage::prepare(Default::default(), &cancel).unwrap();
    let file = package
        .files()
        .iter()
        .find(|file| file.source().name() == "kagari://std/array.kgr")
        .unwrap();
    let mut lowered =
        lower_module_controlled(file.source().clone(), &file.parsed().syntax(), &cancel);
    lowered.module.opaque_types[0].generic_params.clear();
    assert!(
        matches!(install_types(file, &mut lowered, &cancel), Err(PackageError::Annotation { message, .. }) if message.contains("parameter count"))
    );
    assert!(lowered.installed_stdlib.is_none());
}

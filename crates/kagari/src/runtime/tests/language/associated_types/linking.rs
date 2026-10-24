use crate::{engine::KagariEngine, test_support::execution};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::source_database::SourceLayer;

#[test]
fn imported_associated_types_keep_trait_identity_and_interface_bindings() {
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            "pub trait Reader { type Item; fn read(self) -> Self::Item; } pub struct Number { pub val value: i32 } impl Reader for Number { type Item = i32; fn read(self) -> Self::Item { self.value } } pub fn make() -> Number { Number { value: 42 } }",
        ),
        (
            "root",
            "use pkg::model::{Reader, Number, make}; pub struct Output { pub val item: <Number as Reader>::Item } pub fn concrete() -> <Number as Reader>::Item { val output: <Number as Reader>::Item = 0; output } fn read<R: Reader>(r: R) -> R::Item { r.read() } fn read_interface(r: Reader<Item = i32>) -> i32 { r.read() } fn main() -> i32 { read_interface(make()) + read(make()) - 42 + Output { item: concrete() }.item }",
        ),
    ] {
        let path = format!("mem://{name}");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let id = engine
            .set_source(&path, source.into(), SourceLayer::Base)
            .unwrap();
        if name == "root" {
            root = Some(id);
        }
    }
    let root = root.expect("root");
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), root, &Default::default())
        .unwrap();
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    execution::assert_artifact_returns_i32(&engine, artifact, 42);
}

#[test]
fn imported_generic_interfaces_materialize_all_methods_in_the_owning_module() {
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            "pub trait Reader<T> { type Item; fn read(self) -> Self::Item; fn add(self, value: T) -> i32; } pub struct Holder<T> { pub val value: T } impl<T> Reader<i32> for Holder<T> { type Item = T; fn add(self, value: i32) -> i32 { value + 2 } fn read(self) -> T { self.value } }",
        ),
        (
            "root",
            "use pkg::model::{Reader, Holder}; fn boxed<T>(value: Holder<T>) -> Reader<i32, Item = T> { value } fn main() -> i32 { val number = boxed(Holder { value: 20 }); val text: Reader<i32, Item = String> = Holder { value: \"text\" }; if text.read() == \"text\" { number.read() + number.add(20) } else { 0 } }",
        ),
    ] {
        let path = format!("mem://{name}");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let id = engine
            .set_source(&path, source.into(), SourceLayer::Base)
            .unwrap();
        if name == "root" {
            root = Some(id);
        }
    }
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), root.unwrap(), &Default::default())
        .unwrap();
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    let model = artifact
        .records()
        .program
        .modules
        .iter()
        .find(|module| module.identity.path == ["model"])
        .unwrap();
    let tables: Vec<_> = model
        .interface_tables
        .iter()
        .filter(|table| !table.arguments.is_empty())
        .collect();
    assert_eq!(tables.len(), 2);
    assert!(tables.iter().all(|table| table.methods.len() == 2));
    execution::assert_artifact_returns_i32(&engine, artifact, 42);
}

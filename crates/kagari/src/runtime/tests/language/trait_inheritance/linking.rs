use super::execute;
use crate::engine::KagariEngine;
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::source_database::SourceLayer;

#[test]
fn imported_child_bounds_keep_hidden_parent_declarations_and_projections() {
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "parent",
            "pub trait Read<T> { type Item; fn read(self) -> Self::Item; }",
        ),
        (
            "model",
            "use pkg::parent::Read; pub trait Child<T>: Read<T, Item = T> {} pub struct Holder<T> { pub val value: T } impl<T> Read<T> for Holder<T> { type Item = T; fn read(self) -> T { self.value } } impl<T> Child<T> for Holder<T> {} pub fn boxed(x: Holder<i32>) -> Child<i32> { x }",
        ),
        (
            "root",
            "use pkg::model::{Child, Holder, boxed}; fn read<T: Child<i32>>(x: T) -> T::Item { x.read() } fn main() -> i32 { read(Holder { value: 20 }) + boxed(Holder { value: 22 }).read() }",
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
    execute(artifact);
}

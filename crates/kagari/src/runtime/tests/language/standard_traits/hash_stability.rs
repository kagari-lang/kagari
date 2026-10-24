//! Hash results are compared across independently compiled variant layouts.
use crate::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_source::source::SourceFile;

#[test]
fn explicit_enum_hash_is_stable_across_variant_reordering() {
    let mut hashes = Vec::new();
    for variants in ["Empty, Present(Key)", "Present(Key), Empty"] {
        let source = format!(
            "use std::hash::{{Hash}};\nstruct Key {{val id:i32}} impl PartialEq for Key {{fn eq(self,other:Self)->bool {{self.id==other.id}}}} impl Eq for Key {{}} impl Hash for Key {{fn hash(self)->i64 {{self.id.hash()}}}} enum Envelope {{{variants}}} impl PartialEq for Envelope {{fn eq(self,other:Self)->bool {{as_option(self)==as_option(other)}}}} impl Eq for Envelope {{}} impl Hash for Envelope {{fn hash(self)->i64 {{as_option(self).hash()}}}} fn as_option(value:Envelope)->Option<Key> {{match value {{Envelope::Empty=>None,Envelope::Present(key)=>Some(key)}}}} fn main()->i64 {{Envelope::Present(Key{{id:1}}).hash()}}"
        );
        let engine = KagariEngine::default();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("stable-variant.kgr", source),
                Default::default(),
            )
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        hashes.push(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
        );
    }
    assert_eq!(hashes[0], hashes[1]);
}

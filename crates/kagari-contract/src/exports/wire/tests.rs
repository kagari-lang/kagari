use super::*;
use bincode::Options;
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};

#[test]
fn wire_rejects_duplicate_export_records_and_foreign_references() {
    let context = DefinitionContext::new().unwrap();
    let root = ModuleIdentity::single_file("exports");
    let declaration = context
        .intern_child(
            context.intern_root(&root).unwrap(),
            DefinitionKind::Function,
            "f",
            0,
        )
        .unwrap();
    let encoding = context.snapshot().encode([declaration]).unwrap();
    let decoding = encoding.table.clone().decode().unwrap();
    let reference = encoding.reference(declaration).unwrap();
    for corruption in 0..4 {
        let namespace = || NamespaceRecord {
            identity: root.clone(),
            modules: vec![],
            functions: vec![("f".into(), reference)],
        };
        let mut record = ProgramExportsRecord {
            namespaces: vec![namespace()],
            entry: None,
        };
        match corruption {
            0 => record.namespaces.push(namespace()),
            1 => record.namespaces[0].functions.push(("f".into(), reference)),
            2 => record.namespaces[0].modules = vec![("same".into(), root.clone()); 2],
            3 => {
                record.entry = Some(bincode::deserialize(&u32::MAX.to_le_bytes()).unwrap());
            }
            _ => unreachable!(),
        }
        let bytes = bincode::serialize(&record).unwrap();
        let decoded: ProgramExportsRecord = bincode::deserialize(&bytes).unwrap();
        assert!(
            decoded.decode(&decoding, &Default::default()).is_err(),
            "corruption {corruption}"
        );
    }
    // Reject an oversized namespace sequence before reading/allocating its elements.
    let error = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .deserialize::<ProgramExportsRecord>(&((MAX_EXPORT_RECORDS + 1) as u64).to_le_bytes())
        .err()
        .unwrap();
    assert!(error.to_string().contains("export record"), "{error}");
}

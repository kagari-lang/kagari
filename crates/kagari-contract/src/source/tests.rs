use super::*;
use crate::{
    decode_limits::MAX_TABLE_RECORDS,
    source::{
        manifest::{SourceDocumentAnchor, SourceManifest},
        wire::{SourceManifestRecord, SourcePackageRecord},
    },
};
use bincode::{DefaultOptions, Options};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_types::{
    scalar::BuiltinType,
    ty::{Ty, substitution::MAX_TYPE_DEPTH},
};

fn empty() -> SourcePackage {
    SourcePackage {
        executable_fingerprint: 1,
        documents: vec![],
        declarations: vec![],
        executables: vec![],
    }
}

#[test]
fn source_budget_bounds_bytes_tables_type_depth_and_aggregate_type_work() {
    let cancel = CancellationToken::default();
    let mut package = empty();
    package.documents.push(SourceDocument {
        modules: vec![],
        uri: "memory".into(),
        text: "x".repeat(MAX_SOURCE_BYTES),
        fingerprint: 0,
    });
    assert_eq!(
        package.validate_limits(&cancel),
        Err(DefinitionMappingError::LimitExceeded)
    );
    package.documents.clear();
    package
        .documents
        .resize_with(MAX_TABLE_RECORDS + 1, || SourceDocument {
            modules: vec![],
            uri: String::new(),
            text: String::new(),
            fingerprint: 0,
        });
    assert_eq!(
        package.validate_limits(&cancel),
        Err(DefinitionMappingError::LimitExceeded)
    );
    package.documents.clear();
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("bounded"))
        .unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Function, "f", 0)
        .unwrap();
    let mut deep = Ty::Builtin(BuiltinType::I32);
    for _ in 0..MAX_TYPE_DEPTH + 1 {
        deep = Ty::Array(Box::new(deep));
    }
    package.executables.push(ExecutableSource {
        target: ConcreteFunctionIdentity {
            declaration,
            arguments: vec![deep],
        },
        authored: Some(declaration),
        native_binding: None,
        origin: ExecutableOrigin::Named,
    });
    assert_eq!(
        package.validate_limits(&cancel),
        Err(DefinitionMappingError::LimitExceeded)
    );
    package.executables[0].target.arguments =
        vec![Ty::Builtin(BuiltinType::I32); MAX_NESTED_RECORDS + 1];
    assert_eq!(
        package.validate_limits(&cancel),
        Err(DefinitionMappingError::LimitExceeded)
    );
    let mut bytes = 0;
    let mut nodes = MAX_TABLE_RECORDS;
    assert_eq!(
        limits::charge_types(
            &[Ty::Builtin(BuiltinType::I32)],
            &mut bytes,
            &mut nodes,
            &cancel
        ),
        Err(DefinitionMappingError::LimitExceeded)
    );
    let text = "π\r\n".repeat(20_000);
    assert_eq!(
        checked_text_fingerprint(&text, &cancel).unwrap(),
        text_fingerprint(&text)
    );
    cancel.cancel();
    assert_eq!(
        checked_text_fingerprint(&text, &cancel),
        Err(DefinitionMappingError::Cancelled)
    );
    assert_eq!(
        empty().validate_limits(&cancel),
        Err(DefinitionMappingError::Cancelled)
    );
}

#[test]
fn manifest_bounds_identity_authorities_wire_and_cancellation() {
    let cancel = CancellationToken::default();
    let context = DefinitionContext::new().unwrap();
    let module = ModuleIdentity::single_file("anchors");
    let root = context.intern_root(&module).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Function, "f", 0)
        .unwrap();
    let definitions = context.snapshot();
    let document = SourceDocument::new(
        vec![module],
        "src/lib.kgr".into(),
        "// π\r\nfn f() {}".into(),
    );
    let manifest = SourceManifest {
        documents: vec![SourceDocumentAnchor::from_document(&document)],
        declarations: vec![AuthoredDeclaration {
            declaration,
            kind: AuthoredKind::Function,
            document: 0,
            range: Span::new(document.text.find("fn").unwrap(), document.text.len()),
            fingerprint: text_fingerprint("fn f() {}"),
        }],
    };
    manifest.validate(&definitions, &cancel).unwrap();
    let encoding = definitions.encode([declaration]).unwrap();
    let codec = || {
        DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .with_limit(MAX_SOURCE_BYTES as u64)
    };
    let bytes = codec()
        .serialize(&SourceManifestRecord::encode(&manifest, &encoding, &cancel).unwrap())
        .unwrap();
    let decoding = encoding.table.decode().unwrap();
    let decoded = codec()
        .deserialize::<SourceManifestRecord>(&bytes)
        .unwrap()
        .decode(&decoding, &cancel)
        .unwrap();
    decoded.validate(&decoding.table, &cancel).unwrap();
    assert_eq!(decoded.documents, manifest.documents);
    assert_eq!(
        decoded.declarations[0].range,
        manifest.declarations[0].range
    );
    assert_eq!(
        decoding
            .table
            .resolve(decoded.declarations[0].declaration)
            .unwrap()
            .to_path(),
        definitions.resolve(declaration).unwrap().to_path()
    );
    let empty = definitions.encode([]).unwrap().table.decode().unwrap();
    assert!(
        codec()
            .deserialize::<SourceManifestRecord>(&bytes)
            .unwrap()
            .decode(&empty, &cancel)
            .is_err()
    );
    let mut bad = manifest.clone();
    bad.documents.push(bad.documents[0].clone());
    assert!(bad.validate(&definitions, &cancel).is_err());
    let mut bad = manifest.clone();
    bad.declarations.push(bad.declarations[0].clone());
    assert!(bad.validate(&definitions, &cancel).is_err());
    for range in [
        Span::new(1, 1),
        Span::new(4, 3),
        Span::new(0, document.text.len() + 1),
    ] {
        let mut bad = manifest.clone();
        bad.declarations[0].range = range;
        assert!(bad.validate(&definitions, &cancel).is_err());
    }
    let mut bad = manifest.clone();
    bad.declarations[0].document = u32::MAX;
    assert!(bad.validate(&definitions, &cancel).is_err());
    let mut bad = manifest.clone();
    bad.documents[0].byte_length = MAX_SOURCE_BYTES as u64;
    assert_eq!(
        bad.validate_limits(&cancel),
        Err(DefinitionMappingError::LimitExceeded)
    );
    let mut bad = manifest.clone();
    bad.documents[0].modules.resize(
        MAX_NESTED_RECORDS + 1,
        ModuleIdentity::single_file("anchors"),
    );
    assert_eq!(
        bad.validate_limits(&cancel),
        Err(DefinitionMappingError::LimitExceeded)
    );
    let mut oversized = bytes.clone();
    oversized[..8].copy_from_slice(&((MAX_TABLE_RECORDS + 1) as u64).to_le_bytes());
    assert!(
        codec()
            .deserialize::<SourceManifestRecord>(&oversized)
            .is_err()
    );
    cancel.cancel();
    assert_eq!(
        manifest.validate(&definitions, &cancel),
        Err(DefinitionMappingError::Cancelled)
    );
    assert!(matches!(
        SourceManifestRecord::encode(&manifest, &encoding, &cancel),
        Err(DefinitionMappingError::Cancelled)
    ));
    assert!(matches!(
        codec()
            .deserialize::<SourceManifestRecord>(&bytes)
            .unwrap()
            .decode(&decoding, &cancel),
        Err(DefinitionMappingError::Cancelled)
    ));
}

#[test]
fn portable_source_records_remap_identity_and_reject_missing_references_and_counts() {
    let cancel = CancellationToken::default();
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("source"))
        .unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Function, "f", 0)
        .unwrap();
    let definitions = context.snapshot();
    let encoding = definitions.encode([declaration]).unwrap();
    let mut package = empty();
    package.documents.push(SourceDocument::new(
        vec![ModuleIdentity::single_file("source")],
        "source.kgr".into(),
        "// π\r\nfn f() {}".into(),
    ));
    package.declarations.push(AuthoredDeclaration {
        declaration,
        kind: AuthoredKind::Function,
        document: 0,
        range: Span::new(7, 16),
        fingerprint: text_fingerprint("fn f() {}"),
    });
    let codec = || {
        DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .with_limit(MAX_SOURCE_BYTES as u64)
    };
    let encoded = SourcePackageRecord::encode(&package, &encoding, &cancel).unwrap();
    let bytes = codec().serialize(&encoded).unwrap();
    let decoding = encoding.table.decode().unwrap();
    let record: SourcePackageRecord = codec().deserialize(&bytes).unwrap();
    let decoded = record.decode(&decoding, &cancel).unwrap();
    assert_eq!(decoded.documents, package.documents);
    assert_eq!(
        decoding
            .table
            .resolve(decoded.declarations[0].declaration)
            .unwrap()
            .to_path(),
        definitions.resolve(declaration).unwrap().to_path()
    );
    let empty_encoding = definitions.encode([]).unwrap();
    let record: SourcePackageRecord = codec().deserialize(&bytes).unwrap();
    assert!(
        record
            .decode(&empty_encoding.table.decode().unwrap(), &cancel)
            .is_err()
    );
    let mut oversized_document = codec().serialize(&package.documents[0]).unwrap();
    oversized_document[..8].copy_from_slice(&((MAX_NESTED_RECORDS + 1) as u64).to_le_bytes());
    assert!(
        codec()
            .deserialize::<SourceDocument>(&oversized_document)
            .is_err()
    );
    let mut oversized = package.clone();
    oversized.documents[0].modules.resize(
        MAX_NESTED_RECORDS + 1,
        ModuleIdentity::single_file("source"),
    );
    assert_eq!(
        oversized.validate_limits(&cancel),
        Err(DefinitionMappingError::LimitExceeded)
    );
    let mut malformed = codec()
        .serialize(&SourcePackageRecord::encode(&empty(), &empty_encoding, &cancel).unwrap())
        .unwrap();
    // The first vector length follows the executable fingerprint; reject before allocation.
    malformed[8..16].copy_from_slice(&((MAX_TABLE_RECORDS + 1) as u64).to_le_bytes());
    assert!(
        codec()
            .deserialize::<SourcePackageRecord>(&malformed)
            .is_err()
    );
}

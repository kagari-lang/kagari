use super::*;
use crate::{
    bytecode::lower_program_to_bytecode,
    source::{lower::instances::MirLoweringOptions, program::lower_program_to_mir},
};
use kagari_bytecode::artifact::{ArtifactCompatibility, KbcArtifact};
use kagari_common::{
    identity::{ModuleIdentity, PackageId, map::DefinitionContext},
    span::Span,
};
use kagari_contract::{source::ExecutableOrigin, updates::BodyClassification};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::{
    package::{PackageEnvironment, PackageScope},
    source_database::{CapturedSource, SourceDatabase, SourceLayer},
};

fn checked(text: &str) -> CheckedProgram {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("exact.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(kagari_stdlib::catalog::shared().unwrap());
    analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap()
}

#[test]
fn executable_source_anchors_reject_same_document_definition_swaps() {
    let cancel = CancellationToken::default();
    let checked = checked(
        "fn a() -> i32 { 1 } fn b() -> i32 { 2 } struct A { val x: i32 } struct B { val y: i32 } fn unused<T>(x: T) -> T { x } fn other<T>(y: T) -> T { y }",
    );
    let bytecode = compile(&checked);
    let package = build_source_package(&checked, &bytecode, &cancel).unwrap();
    let artifact =
        KbcArtifact::from_verified(bytecode.clone(), Default::default(), &cancel).unwrap();
    let artifact = KbcArtifact::with_source_package(artifact, &package, &cancel).unwrap();
    assert!(artifact.records().debug.is_none());
    assert!(artifact.records().portable_mir.is_none());
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let fresh = DefinitionContext::new().unwrap();
    let loaded = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&fresh, &cancel)
        .unwrap()
        .into_records()
        .into_verified(&ArtifactCompatibility::default(), &fresh, &cancel)
        .unwrap();
    let (loaded_program, _, loaded_package) = loaded.into_parts();
    let loaded_package = loaded_package.unwrap();
    for (bytecode, package) in [(&bytecode, &package), (&loaded_program, &loaded_package)] {
        for (left, right) in [("a", "b"), ("A", "B"), ("unused", "other")] {
            let a = named(package, left);
            let b = named(package, right);
            assert_eq!(a.document, b.document);
            let mut records = package.records().clone();
            for record in &mut records.declarations {
                if record.declaration == a.declaration {
                    record.range = b.range;
                    record.fingerprint = b.fingerprint;
                } else if record.declaration == b.declaration {
                    record.range = a.range;
                    record.fingerprint = a.fingerprint;
                }
            }
            let forged =
                DefinitionMetadata::checked(package.definitions().clone(), records, &cancel)
                    .unwrap();
            assert!(
                matches!(
                    VerifiedSourcePackage::validate(forged, bytecode, &cancel),
                    Err(SourcePackageError::DeclarationMismatch)
                ),
                "accepted swapped {left}/{right} source"
            );
        }
    }
    // An attacker can re-encode the unsealed envelope, including its payload hash.
    // Executable-owned anchors must still reject a forged attachment on admission.
    let mut forged = artifact.records().clone();
    let a = named(&package, "a");
    let b = named(&package, "b");
    let declarations = &mut forged.source.as_mut().unwrap().declarations;
    let record = declarations
        .iter_mut()
        .find(|record| record.declaration == a.declaration)
        .unwrap();
    record.range = b.range;
    record.fingerprint = b.fingerprint;
    let bytes = forged.to_bytes(artifact.definitions()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let forged = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&context, &cancel)
        .unwrap();
    assert!(
        forged
            .into_records()
            .into_verified(&ArtifactCompatibility::default(), &context, &cancel)
            .is_err()
    );
}

#[test]
fn equal_display_and_text_keep_distinct_module_origins_and_inline_coordinates() {
    let cancel = CancellationToken::default();
    let module = |name: &str| ModuleIdentity {
        package: PackageId(name.into()),
        path: vec![],
    };
    let a = module("a");
    let b = module("b");
    let root = module("app");
    let shared =
        "// π😀\r\nmod inner { fn hidden() -> i32 { 7 } }\r\npub fn value() -> i32 { 42 }\r\n";
    let packages = PackageEnvironment::new(
        [
            (
                root.package.clone(),
                PackageScope {
                    dependencies: [
                        ("a".into(), a.package.clone()),
                        ("b".into(), b.package.clone()),
                    ]
                    .into(),
                    selection: "test".into(),
                },
            ),
            (
                a.package.clone(),
                PackageScope {
                    dependencies: Default::default(),
                    selection: "test".into(),
                },
            ),
            (
                b.package.clone(),
                PackageScope {
                    dependencies: Default::default(),
                    selection: "test".into(),
                },
            ),
        ]
        .into(),
    )
    .unwrap();
    let mut sources = SourceDatabase::default();
    let snapshot = sources
        .replace_captured(
            vec![
                CapturedSource {
                    key: "host://a".into(),
                    display_name: "src/lib.kgr".into(),
                    module: a.clone(),
                    text: shared.into(),
                },
                CapturedSource {
                    key: "host://b".into(),
                    display_name: "src/lib.kgr".into(),
                    module: b.clone(),
                    text: shared.into(),
                },
                CapturedSource {
                    key: "host://root".into(),
                    display_name: "main.kgr".into(),
                    module: root.clone(),
                    text: "fn main() -> i32 { a::value() + b::value() }".into(),
                },
            ],
            packages,
            &cancel,
        )
        .unwrap();
    let file = snapshot.module(&root).unwrap().id();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(kagari_stdlib::catalog::shared().unwrap());
    let checked = analysis
        .snapshot(snapshot, &cancel)
        .unwrap()
        .check_program(file, &cancel)
        .unwrap();
    let bytecode = compile(&checked);
    let package = build_source_package(&checked, &bytecode, &cancel).unwrap();
    let a_doc = package
        .records()
        .documents
        .iter()
        .position(|doc| doc.modules.contains(&a))
        .unwrap();
    let b_doc = package
        .records()
        .documents
        .iter()
        .position(|doc| doc.modules.contains(&b))
        .unwrap();
    assert_ne!(a_doc, b_doc);
    for index in [a_doc, b_doc] {
        let document = &package.records().documents[index];
        assert_eq!(document.uri, "src/lib.kgr");
        assert_eq!(document.text, shared);
        assert_eq!(document.modules.len(), 2);
        let hidden = package
            .records()
            .declarations
            .iter()
            .find(|record| {
                record.document as usize == index
                    && package
                        .definitions()
                        .resolve(record.declaration)
                        .unwrap()
                        .last()
                        .unwrap()
                        .name
                        == "hidden"
            })
            .unwrap();
        assert_eq!(hidden.range.start, shared.find("fn hidden").unwrap());
        assert_eq!(text(&package, hidden), "fn hidden() -> i32 { 7 }");
    }
    assert_eq!(
        package.records().documents[a_doc].fingerprint,
        package.records().documents[b_doc].fingerprint
    );
    let validate = |records| {
        VerifiedSourcePackage::validate(
            DefinitionMetadata::checked(package.definitions().clone(), records, &cancel).unwrap(),
            &bytecode,
            &cancel,
        )
    };
    // Identical bytes and valid offsets cannot transfer a declaration to another module's document.
    let mut swapped = package.records().clone();
    swapped
        .declarations
        .iter_mut()
        .find(|record| record.document as usize == a_doc)
        .unwrap()
        .document = b_doc as u32;
    assert!(matches!(
        validate(swapped),
        Err(SourcePackageError::InvalidDocument)
    ));
    // The same module-scoped origin cannot acquire a second physical document,
    // even with distinct, internally consistent bytes and fingerprints.
    let mut duplicate = package.records().clone();
    let mut extra = duplicate.documents[a_doc].clone();
    extra.text.push_str("// different bytes");
    extra.fingerprint = checked_text_fingerprint(&extra.text, &cancel).unwrap();
    duplicate.documents.push(extra);
    assert!(matches!(
        validate(duplicate),
        Err(SourcePackageError::InvalidDocument)
    ));
    let mut duplicate = package.records().clone();
    duplicate.documents[a_doc].modules.push(a.clone());
    assert!(matches!(
        validate(duplicate),
        Err(SourcePackageError::InvalidDocument)
    ));
    let artifact = KbcArtifact::from_verified(bytecode, Default::default(), &cancel).unwrap();
    let artifact = KbcArtifact::with_source_package(artifact, &package, &cancel).unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let loaded = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&context, &cancel)
        .unwrap()
        .into_records()
        .into_verified(&ArtifactCompatibility::default(), &context, &cancel)
        .unwrap();
    assert_eq!(
        loaded.source_package().unwrap().records().documents,
        package.records().documents
    );
}

#[test]
fn intrinsic_helpers_have_no_fabricated_authored_owner_and_round_trip() {
    let checked = checked(
        r#"
use std::fmt::Debug;
use std::hash::Hash;
fn first() -> i32 { 7 }
fn main() -> String { val view: Debug = 42; view.debug() }
fn hashed<T: Hash>(value: T) -> i64 { val boxed: Hash = value; boxed.hash() }
fn tuple() -> i64 { hashed((42, 3)) }
"#,
    );
    let cancel = CancellationToken::default();
    let bytecode = compile(&checked);
    let package = build_source_package(&checked, &bytecode, &cancel).unwrap();
    let intrinsic = package
        .records()
        .executables
        .iter()
        .filter(|record| record.origin == ExecutableOrigin::Intrinsic)
        .collect::<Vec<_>>();
    assert!(!intrinsic.is_empty());
    for record in &intrinsic {
        assert!(record.authored.is_none());
        assert!(bytecode.program().update_evidence.bodies.iter().any(|body| matches!(&body.classification, BodyClassification::IntrinsicAdapter(target) if target == &record.target)));
    }
    assert!(
        package
            .records()
            .executables
            .iter()
            .any(|record| record.origin == ExecutableOrigin::Native)
    );
    for record in package
        .records()
        .executables
        .iter()
        .filter(|record| record.origin == ExecutableOrigin::Native)
    {
        assert!(
            bytecode
                .program()
                .modules
                .iter()
                .flat_map(|module| &module.native_imports)
                .any(|native| native.instance == record.target
                    && Some(native.binding) == record.native_binding)
        );
    }
    let mut wrong_binding = package.records().clone();
    wrong_binding
        .executables
        .iter_mut()
        .find(|record| record.origin == ExecutableOrigin::Native)
        .unwrap()
        .native_binding = Some(named(&package, "first").declaration);
    assert!(matches!(
        VerifiedSourcePackage::validate(
            DefinitionMetadata::checked(package.definitions().clone(), wrong_binding, &cancel)
                .unwrap(),
            &bytecode,
            &cancel
        ),
        Err(SourcePackageError::OriginMismatch)
    ));
    // Intrinsic origin is checked evidence, not permission to point at the requesting
    // function (or the unrelated first function used as the lowering context).
    let mut forged = package.records().clone();
    forged
        .executables
        .iter_mut()
        .find(|record| record.origin == ExecutableOrigin::Intrinsic)
        .unwrap()
        .authored = Some(named(&package, "first").declaration);
    assert!(matches!(
        VerifiedSourcePackage::validate(
            DefinitionMetadata::checked(package.definitions().clone(), forged, &cancel).unwrap(),
            &bytecode,
            &cancel
        ),
        Err(SourcePackageError::OriginMismatch)
    ));
    let artifact = KbcArtifact::from_verified(bytecode, Default::default(), &cancel).unwrap();
    let artifact = KbcArtifact::with_source_package(artifact, &package, &cancel).unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let loaded = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&context, &cancel)
        .unwrap()
        .into_records()
        .into_verified(&ArtifactCompatibility::default(), &context, &cancel)
        .unwrap();
    let (_, _, source) = loaded.into_parts();
    let source = source.unwrap();
    assert_eq!(
        source
            .records()
            .executables
            .iter()
            .filter(
                |record| record.origin == ExecutableOrigin::Intrinsic && record.authored.is_none()
            )
            .count(),
        intrinsic.len()
    );
    assert!(
        source
            .records()
            .executables
            .iter()
            .filter(|record| record.origin == ExecutableOrigin::Native)
            .all(|record| record.native_binding.is_some())
    );
    assert_eq!(
        text(&source, named(&source, "main")),
        "fn main() -> String { val view: Debug = 42; view.debug() }"
    );
}

#[test]
fn source_local_adapters_keep_authored_owner_and_cannot_claim_intrinsic_origin() {
    let checked = checked(
        r#"
struct Number { val value: i32 }
impl Fn<(i32,)> for Number {
    type Output = i32;
    fn call(self, args: (i32,)) -> i32 { self.value + args[0] }
}
fn convert() -> fn(i32) -> i32 { Number { value: 2 } }
"#,
    );
    let cancel = CancellationToken::default();
    let bytecode = compile(&checked);
    let source = build_source_package(&checked, &bytecode, &cancel).unwrap();
    let adapter = source
        .records()
        .executables
        .iter()
        .find(|record| record.origin == ExecutableOrigin::Adapter)
        .unwrap();
    assert_eq!(
        adapter.authored,
        Some(named(&source, "convert").declaration)
    );
    assert_eq!(
        text(&source, named(&source, "convert")),
        "fn convert() -> fn(i32) -> i32 { Number { value: 2 } }"
    );
    let mut forged = source.records().clone();
    let record = forged
        .executables
        .iter_mut()
        .find(|record| record.target == adapter.target)
        .unwrap();
    record.origin = ExecutableOrigin::Intrinsic;
    record.authored = None;
    assert!(matches!(
        VerifiedSourcePackage::validate(
            DefinitionMetadata::checked(source.definitions().clone(), forged, &cancel).unwrap(),
            &bytecode,
            &cancel
        ),
        Err(SourcePackageError::OriginMismatch)
    ));
    // Even editing the executable certificate cannot make a source-local helper
    // pass the module-owned intrinsic classification proof.
    let mut program = bytecode.program().clone();
    program
        .update_evidence
        .bodies
        .iter_mut()
        .find(|body| body.requirement.origin == adapter.target)
        .unwrap()
        .classification = BodyClassification::IntrinsicAdapter(adapter.target.clone());
    assert!(
        VerifiedBytecodeProgram::from_metadata(
            DefinitionMetadata::checked(bytecode.definitions().clone(), program, &cancel).unwrap(),
            &cancel
        )
        .is_err()
    );
}

#[test]
fn foreign_defaults_and_materialized_generic_applications_keep_authored_owner() {
    let mut sources = SourceDatabase::default();
    let mut insert = |name: &str, text: &str| {
        sources
            .bind_module(
                name,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        sources.set(name, text.into(), SourceLayer::Base).unwrap()
    };
    insert(
        "model",
        "pub trait Read { fn read(self) -> i32; fn again(self) -> i32 { self.read() } } pub fn identity<T>(x: T) -> T { x }",
    );
    let root = insert(
        "root",
        "use pkg::model::{Read, identity}; struct Holder { val value: i32 } impl Read for Holder { fn read(self) -> i32 { self.value } } fn main() -> i32 { val x: Read = Holder { value: 7 }; identity(x.again()) }",
    );
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(kagari_stdlib::catalog::shared().unwrap());
    let checked = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let bytecode = compile(&checked);
    let package = build_source_package(&checked, &bytecode, &Default::default()).unwrap();
    let again = named(&package, "again");
    assert_eq!(
        text(&package, again),
        "fn again(self) -> i32 { self.read() }"
    );
    assert!(package.records().executables.iter().any(|record| {
        record.authored == Some(again.declaration)
            && package
                .definitions()
                .resolve(record.target.declaration)
                .unwrap()
                .module()
                .path
                == ["root"]
    }));
    assert!(
        package
            .records()
            .executables
            .iter()
            .any(
                |record| record.authored == Some(named(&package, "identity").declaration)
                    && !record.target.arguments.is_empty()
            )
    );
}

fn compile(checked: &CheckedProgram) -> VerifiedBytecodeProgram {
    let options = MirLoweringOptions::default();
    let mir = lower_program_to_mir(checked, &options).unwrap();
    lower_program_to_bytecode(&mir, &options.cancel).unwrap()
}

fn named<'a>(package: &'a VerifiedSourcePackage, name: &str) -> &'a AuthoredDeclaration {
    package
        .records()
        .declarations
        .iter()
        .find(|record| {
            package
                .definitions()
                .resolve(record.declaration)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == name)
        })
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn text<'a>(package: &'a VerifiedSourcePackage, record: &AuthoredDeclaration) -> &'a str {
    &package.records().documents[record.document as usize].text
        [record.range.start..record.range.end]
}

#[test]
fn exact_definitions_keep_private_unused_generic_local_inline_unicode_and_crlf() {
    let source = "// π😀\r\nfn unused<T>(x: T) -> T { x }\r\nmod inner { fn hidden() -> i32 { 7 } }\r\nfn main() -> i32 { fn local() -> i32 { 3 } local() }\r\n";
    let checked = checked(source);
    let bytecode = compile(&checked);
    let package = build_source_package(&checked, &bytecode, &Default::default()).unwrap();
    assert_eq!(
        text(&package, named(&package, "unused")),
        "fn unused<T>(x: T) -> T { x }"
    );
    assert_eq!(
        text(&package, named(&package, "hidden")),
        "fn hidden() -> i32 { 7 }"
    );
    assert_eq!(
        text(&package, named(&package, "local")),
        "fn local() -> i32 { 3 }"
    );
    assert!(text(&package, named(&package, "main")).contains("fn local()"));
    let document = &package.records().documents[named(&package, "hidden").document as usize];
    assert_eq!(document.text, source);
    assert_eq!(
        named(&package, "hidden").range.start,
        source.find("fn hidden").unwrap()
    );
    assert_eq!(
        package
            .definitions()
            .resolve(named(&package, "hidden").declaration)
            .unwrap()
            .module()
            .path,
        ["inner"]
    );
}

#[test]
fn generated_origins_and_source_only_artifact_round_trip() {
    let checked = checked(
        "fn closure() -> fn(i32) -> i32 { |x: i32| x } async fn work(x: i32) -> i32 { x } fn main() { val f = async { 1 }; } static val VALUE: i32 = 7;",
    );
    let bytecode = compile(&checked);
    let package = build_source_package(&checked, &bytecode, &Default::default()).unwrap();
    for (origin, owner) in [
        (ExecutableOrigin::Closure, "closure"),
        (ExecutableOrigin::Resume, "work"),
        (ExecutableOrigin::StaticInitializer, "VALUE"),
    ] {
        assert!(
            package
                .records()
                .executables
                .iter()
                .any(|record| record.origin == origin
                    && record.authored == Some(named(&package, owner).declaration)),
            "missing {origin:?}"
        );
    }
    let cancel = CancellationToken::default();
    let artifact = KbcArtifact::from_verified(bytecode, Default::default(), &cancel).unwrap();
    assert!(artifact.records().source.is_none());
    let artifact = KbcArtifact::with_source_package(artifact, &package, &cancel).unwrap();
    assert!(artifact.records().debug.is_none());
    assert!(artifact.records().portable_mir.is_none());
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
    let context = DefinitionContext::new().unwrap();
    let decoded = decoded
        .import_into(&context, &cancel)
        .unwrap()
        .into_records()
        .into_verified(&ArtifactCompatibility::default(), &context, &cancel)
        .unwrap();
    let (_, mir, retained) = decoded.into_parts();
    assert!(mir.is_none());
    let retained = retained.unwrap();
    assert_eq!(
        text(&retained, named(&retained, "work")),
        "async fn work(x: i32) -> i32 { x }"
    );
}

#[test]
fn rejects_mismatched_products_ranges_documents_and_origin_forgery() {
    let checked = checked("// π\r\nfn main() -> i32 { 1 }");
    let bytecode = compile(&checked);
    let package = build_source_package(&checked, &bytecode, &Default::default()).unwrap();
    let cancel = CancellationToken::default();
    let validate = |records| {
        VerifiedSourcePackage::validate(
            DefinitionMetadata::checked(package.definitions().clone(), records, &cancel).unwrap(),
            &bytecode,
            &cancel,
        )
    };
    let main = package
        .records()
        .declarations
        .iter()
        .position(|record| record.declaration == named(&package, "main").declaration)
        .unwrap();
    let mut bad = package.records().clone();
    bad.executable_fingerprint ^= 1;
    assert!(matches!(
        validate(bad),
        Err(SourcePackageError::ExecutableMismatch)
    ));
    for range in [Span::new(4, 5), Span::new(0, usize::MAX), Span::new(0, 2)] {
        let mut bad = package.records().clone();
        bad.declarations[main].range = range;
        assert!(matches!(
            validate(bad),
            Err(SourcePackageError::InvalidDocument)
        ));
    }
    let mut bad = package.records().clone();
    bad.documents[bad.declarations[main].document as usize]
        .text
        .push(' ');
    assert!(matches!(
        validate(bad),
        Err(SourcePackageError::InvalidDocument)
    ));
    let mut bad = package.records().clone();
    bad.declarations.remove(main);
    assert!(validate(bad).is_err());
    let mut bad = package.records().clone();
    bad.executables[0].origin = ExecutableOrigin::Closure;
    assert!(matches!(
        validate(bad),
        Err(SourcePackageError::OriginMismatch)
    ));
    let other = self::checked("fn main() -> i32 { 2 }");
    assert!(build_source_package(&other, &bytecode, &cancel).is_err());
    // Identical function tokens do not excuse a different private declaration contract.
    let original = self::checked("struct Hidden { val value: i32 } fn main() {}");
    let changed = self::checked("struct Hidden { val value: i64 } fn main() {}");
    let bytecode = compile(&original);
    assert!(matches!(
        build_source_package(&changed, &bytecode, &cancel),
        Err(SourcePackageError::DeclarationMismatch)
    ));
}

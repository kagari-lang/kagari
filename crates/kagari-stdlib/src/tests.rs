use super::*;
use kagari_common::cancellation::CancellationToken;
use kagari_syntax::{ast::AstNode, kind::SyntaxKind, parser::ParseLimits};

fn prepare(text: &'static str) -> Result<ParsedStdlibPackage, PackageError> {
    ParsedStdlibPackage::prepare_sources(
        &[BundledSource {
            module: "sample",
            uri: "kagari://std/sample.kgr",
            text,
        }],
        ParseLimits::default(),
        &CancellationToken::default(),
    )
}

#[test]
fn bundled_sources_preserve_exact_text_spans_and_module_identity() {
    let package = ParsedStdlibPackage::prepare(Default::default(), &Default::default()).unwrap();
    assert_eq!(package.files().len(), bundled_sources().len());
    for (file, bundled) in package.files().iter().zip(bundled_sources()) {
        let source = file.source();
        assert_eq!(source.text(), bundled.text());
        assert_eq!(source.name(), bundled.uri());
        assert_eq!(source.module_identity().package, *package.identity());
        assert_eq!(source.module_identity().path, [bundled.module()]);
        assert_eq!(file.parsed().syntax().syntax().to_string(), source.text());
        assert!(file.parsed().diagnostics().is_empty());
        for site in file.declarations() {
            assert!(source.span(site.span).is_some());
            for marker in &site.markers {
                let text = &source.text()[marker.span.start..marker.span.end];
                assert!(text.starts_with("#["));
                assert!(text.contains(&marker.binding));
            }
        }
    }
    let again = ParsedStdlibPackage::prepare(Default::default(), &Default::default()).unwrap();
    assert_eq!(again.fingerprint(), package.fingerprint());
    assert_eq!(again.identity(), package.identity());
}

#[test]
fn preparation_retains_script_bodies_and_unresolved_types() {
    let text = "/// docs\n#[intrinsic(Example)] pub fn native<T>(x: T) -> Unknown<T>;\npub fn script() -> i32 { 42 }\npub type Opaque<T>;";
    let package = prepare(text).unwrap();
    let file = &package.files()[0];
    let sites = file.declarations();
    assert_eq!(sites.len(), 3);
    assert_eq!(sites[0].markers[0].binding, "Example");
    assert_eq!(sites[0].body_span, None);
    let body = sites[1].body_span.unwrap();
    assert_eq!(&text[body.start..body.end], "{ 42 }");
    assert_eq!(sites[2].kind, SyntaxKind::AssociatedType);
    // Unknown type/binding names are semantic input for HIR, not package errors.
    assert_eq!(file.source().text(), text);
}

#[test]
fn malformed_annotations_and_syntax_do_not_publish_a_package() {
    for text in [
        "#[intrinsic(A)] #[intrinsic(B)] pub fn f();",
        "#[intrinsic(A)] #[numeric(B)] pub fn f();",
        "#[intrinsic()] pub fn f();",
        "#[intrinsic(A, B)] pub fn f();",
        "#[intrinsic(binding = A)] pub fn f();",
        "#[intrinsic(42)] pub fn f();",
    ] {
        assert!(
            matches!(prepare(text), Err(PackageError::Annotation { .. })),
            "{text}"
        );
    }
    assert!(matches!(
        prepare("pub fn ("),
        Err(PackageError::Syntax { .. })
    ));
}

#[test]
fn cancellation_and_limits_fail_without_partial_success() {
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        ParsedStdlibPackage::prepare(Default::default(), &cancel),
        Err(PackageError::Cancelled)
    ));
    assert!(matches!(
        ParsedStdlibPackage::prepare(
            ParseLimits {
                max_nesting: 1,
                ..Default::default()
            },
            &Default::default()
        ),
        Err(PackageError::Syntax { .. })
    ));
    assert!(ParsedStdlibPackage::prepare(Default::default(), &Default::default()).is_ok());
}

#[test]
fn fingerprints_include_exact_source_and_manifest_rejects_duplicates() {
    assert_ne!(
        prepare("pub fn a();").unwrap().fingerprint(),
        prepare("pub fn a();\n").unwrap().fingerprint()
    );
    let entry = BundledSource {
        module: "a",
        uri: "kagari://std/a.kgr",
        text: "pub fn a();",
    };
    assert!(matches!(
        ParsedStdlibPackage::prepare_sources(
            &[entry, entry],
            Default::default(),
            &Default::default()
        ),
        Err(PackageError::Manifest(_))
    ));
}

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
fn bundled_public_declarations_keep_their_documentation() {
    let package = ParsedStdlibPackage::prepare(Default::default(), &Default::default()).unwrap();
    for file in package.files() {
        for node in file.parsed().syntax().syntax().descendants() {
            let kind = node.kind();
            if !matches!(
                kind,
                SyntaxKind::FnDef
                    | SyntaxKind::MethodDef
                    | SyntaxKind::TraitDef
                    | SyntaxKind::EnumDef
                    | SyntaxKind::Variant
                    | SyntaxKind::AssociatedType
            ) || (kind == SyntaxKind::AssociatedType
                && node
                    .parent()
                    .is_some_and(|parent| parent.kind() == SyntaxKind::ImplBlock))
            {
                continue;
            }
            let range = node.text_range();
            let site = file
                .declarations()
                .iter()
                .find(|site| {
                    site.span.start == usize::from(range.start())
                        && site.span.end == usize::from(range.end())
                })
                .unwrap();
            assert!(
                !site.documentation.is_empty(),
                "undocumented {}: {:?}",
                file.source().name(),
                site.span
            );
        }
    }
}

#[test]
fn preparation_retains_script_bodies_and_unresolved_types() {
    let text = "/// docs\n#[intrinsic(Example)] pub fn native<T>(x: T) -> Unknown<T>;\npub fn script() -> i32 { 42 }\npub type Opaque<T>;";
    let package = prepare(text).unwrap();
    let file = &package.files()[0];
    let sites = file.declarations();
    assert_eq!(sites.len(), 3);
    assert_eq!(sites[0].markers[0].binding, "Example");
    assert_eq!(sites[0].documentation, "docs");
    assert_eq!(
        sites[0].written_signature,
        "#[intrinsic(Example)] pub fn native<T>(x: T) -> Unknown<T>;"
    );
    assert_eq!(sites[0].body_span, None);
    let body = sites[1].body_span.unwrap();
    assert_eq!(&text[body.start..body.end], "{ 42 }");
    assert_eq!(sites[2].kind, SyntaxKind::AssociatedType);
    // Unknown type/binding names are semantic input for HIR, not package errors.
    assert_eq!(file.source().text(), text);
}

#[test]
fn declaration_documentation_retains_markdown_unicode_and_source_spelling() {
    let text = "/// Optional 文本.\r\n///\r\n/// ```kgr\r\n/// Some(1)\r\n/// ```\r\npub enum Maybe<T> {\r\n    /// Present value.\r\n    Some(T),\r\n}\r\ntrait View {\r\n    /// Read the value.\r\n    fn read(self) -> i32;\r\n}\r\n";
    let package = prepare(text).unwrap();
    let sites = package.files()[0].declarations();
    let enumeration = sites
        .iter()
        .find(|site| site.kind == SyntaxKind::EnumDef)
        .unwrap();
    assert_eq!(
        enumeration.documentation,
        "Optional 文本.\n\n```kgr\nSome(1)\n```"
    );
    assert_eq!(
        enumeration.written_signature,
        text[enumeration.span.start..enumeration.span.end]
    );
    let variant = sites
        .iter()
        .find(|site| site.kind == SyntaxKind::Variant)
        .unwrap();
    assert_eq!(variant.documentation, "Present value.");
    let method = sites
        .iter()
        .find(|site| site.kind == SyntaxKind::MethodDef)
        .unwrap();
    assert_eq!(method.documentation, "Read the value.");
    assert_eq!(method.written_signature, "fn read(self) -> i32;");
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
        "trait T { #[method_policy(Final)] #[method_policy(Overridable)] fn f(self); }",
        "trait T { #[method_policy()] fn f(self); }",
        "trait T { #[method_policy(Final, Overridable)] fn f(self); }",
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

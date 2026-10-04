use super::*;
use crate::{native::render::declaration_source, tests::native::module};
use kagari_stdlib::{catalog as foundation_catalog, catalog};

#[test]
fn native_view_must_match_authoritative_signatures_bounds_members_and_visibility() {
    let module = module();
    let mut providers = catalog::shared();
    providers.push(module.clone());
    let original = declaration_source(&module, &foundation_catalog::shared()).unwrap();
    for (from, to) in [
        ("-> i32", "-> i64"),
        ("pub trait NativeRead", "trait NativeRead"),
        ("fn fixed", "fn different"),
        ("core::hash::Hash", "core::cmp::Eq"),
        ("value0: T0", "value0: i32"),
    ] {
        let mut source = original.clone();
        assert!(source.text.contains(from), "{from}: {}", source.text);
        source.text = source.text.replace(from, to);
        let error = import_source(
            &module,
            &providers,
            &source,
            Default::default(),
            &Default::default(),
        )
        .unwrap_err();
        assert!(
            error.0.contains("differs from authoritative registration"),
            "{error}"
        );
    }
}

#[test]
fn native_view_trivia_does_not_change_checked_binding_and_default_ownership() {
    let module = module();
    let mut source = declaration_source(&module, &foundation_catalog::shared()).unwrap();
    source.text = format!("// Tooling overlay.\n{}", source.text);
    let (_, lowered) = import_source(
        &module,
        &catalog::shared()
            .into_iter()
            .chain([module.clone()])
            .collect::<Vec<_>>(),
        &source,
        Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(lowered.module.traits.len(), 1);
    assert_eq!(lowered.module.traits[0].methods.len(), 2);
    assert!(
        lowered.module.traits[0]
            .methods
            .iter()
            .all(|method| method.has_default)
    );
    assert_eq!(lowered.native_functions.len(), 6);
}

#[test]
fn one_source_analysis_reports_invalid_registration_as_an_error() {
    let mut invalid = module().as_ref().clone();
    invalid.functions.push(invalid.functions[0].clone());
    let source = SourceFile::new("invalid-registration.kgr", "fn main() {}");
    let result = crate::analyze_source(&source, vec![Arc::new(invalid)]);
    assert!(matches!(
        result,
        Err(crate::analysis::error::AnalysisError::NativeApi(_))
    ));
}

#[test]
fn core_roles_use_complete_parsed_modules_with_attribute_trivia_and_docs() {
    let providers = catalog::shared();
    let module = providers
        .iter()
        .find(|module| {
            module.identity.package.0 == "kagari-core" && module.identity.path == ["iter"]
        })
        .unwrap();
    let mut source = declaration_source(module, &providers).unwrap();
    source.text = source
        .text
        .replace("#[lang = \"iterator\"]", "#[ lang\n = \"iterator\" ]");
    source.text = source.text.replace(
        "pub trait Iterator {",
        "pub trait Iterator {\n\n    // #[lang = \"forged\"] in a comment is not an attribute.\n",
    );
    source.text = format!(
        "//! # Module\n//!\n//! ```kgr\n//! #[lang = \"comment_only\"]\n//! ```\n{}",
        source.text
    );
    let (parsed, lowered) = import_source(
        module,
        &providers,
        &source,
        Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        parsed.syntax().module_documentation().lines().next(),
        Some("# Module")
    );
    let iterator = lowered
        .module
        .traits
        .iter()
        .find(|item| item.name == "Iterator")
        .unwrap();
    assert!(iterator.methods.iter().any(|method| method.name == "next"));
    assert!(lowered.language_foundation);
    let id = module.definition(DefinitionKind::Trait, "Iterator");
    assert!(lowered.registered_traits.contains_key(&id));
    let forged = source.text.replace("\"iterator\"", "\"iterable\"");
    source.text = forged;
    assert!(
        import_source(
            module,
            &providers,
            &source,
            Default::default(),
            &Default::default()
        )
        .unwrap_err()
        .0
        .contains("differs from authoritative registration")
    );
}

#[test]
fn malformed_or_mismatched_core_source_never_installs_a_native_parse() {
    let providers = catalog::shared();
    let module = providers
        .iter()
        .find(|module| {
            module.identity.package.0 == "kagari-core" && module.identity.path == ["cmp"]
        })
        .unwrap();
    let original = declaration_source(module, &providers).unwrap();
    for text in [
        original.text.replace("fn eq", "fn unequal"),
        format!("{}\npub trait Broken {{", original.text),
    ] {
        let source = DeclarationSource {
            text,
            ..original.clone()
        };
        assert!(
            import_source(
                module,
                &providers,
                &source,
                Default::default(),
                &Default::default()
            )
            .is_err()
        );
    }
}

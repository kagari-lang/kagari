use super::*;
use crate::{
    native::{adoption::adopt, render::declaration_source},
    tests::native::module,
};
use kagari_common::identity::map::DefinitionContext;
use kagari_stdlib::{catalog as foundation_catalog, catalog};
use kagari_types::{
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

#[test]
fn registered_nested_readonly_results_preserve_access_during_source_analysis() {
    let mut providers = catalog::shared().unwrap();
    providers.push(module());
    let mut providers = adopt(
        &providers,
        &DefinitionContext::new().unwrap(),
        &Default::default(),
    )
    .unwrap();
    let (definitions, mut declaration) = providers.pop().unwrap().as_ref().clone().into_parts();
    let function = declaration
        .functions
        .iter_mut()
        .find(|item| item.name == "echo")
        .unwrap();
    function.generic_params.clear();
    function.params.clear();
    function.return_type = Ty::Tuple(vec![Ty::Trait(NominalTy {
        declaration: definitions
            .lookup(&kagari_stdlib::identity::trait_id("Map"))
            .unwrap(),
        arguments: vec![Ty::Builtin(BuiltinType::I32), Ty::Builtin(BuiltinType::I32)],
        associated_types: Default::default(),
    })]);
    let module = Arc::new(
        DefinitionMetadata::checked(definitions, declaration, &Default::default()).unwrap(),
    );
    providers.push(module);
    for (operation, accepted) in [("get(1)", true), ("insert(1, 2)", false)] {
        let source = SourceFile::new(
            "nested-access.kgr",
            format!("fn main() {{ demo::native::echo()[0].{operation}; }}"),
        );
        let analysis = crate::analyze_source(&source, providers.clone()).unwrap();
        assert_eq!(
            analysis.diagnostics().is_empty(),
            accepted,
            "{:?}",
            analysis.diagnostics()
        );
    }
}

#[test]
fn native_view_must_match_authoritative_signatures_bounds_members_and_visibility() {
    let module = module();
    let mut providers = catalog::shared().unwrap();
    providers.push(module.clone());
    let original = declaration_source(&module, &foundation_catalog::shared().unwrap()).unwrap();
    for (from, to) in [
        ("-> i32", "-> i64"),
        ("pub trait NativeRead", "trait NativeRead"),
        ("fn fixed", "fn different"),
        ("default_read::<Self>(self)", "default_fixed::<Self>(self)"),
        ("default_read::<Self>(self)", "default_read::<Self>()"),
        (
            "default_read::<Self>(self)",
            "default_read::<Self>(self, self)",
        ),
        ("{\n        default_read::<Self>(self)\n    }", ";"),
        ("core::hash::Hash", "core::cmp::Eq"),
        ("value0: T", "value0: i32"),
    ] {
        let mut source = original.clone();
        assert!(source.text.contains(from), "{from}: {}", source.text);
        source.text = source.text.replace(from, to);
        let error = import_source(
            &module,
            &providers,
            &source,
            "/native-declarations",
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
    let mut providers = catalog::shared().unwrap();
    providers.push(module());
    let providers = adopt(
        &providers,
        &DefinitionContext::new().unwrap(),
        &Default::default(),
    )
    .unwrap();
    let module = providers.last().unwrap();
    let mut source = declaration_source(module, &providers).unwrap();
    source.text = format!("// Tooling overlay.\n{}", source.text);
    let (_, lowered) = import_source(
        module,
        &providers,
        &source,
        "/native-declarations",
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
    let (definitions, mut invalid) = module().as_ref().clone().into_parts();
    invalid.functions.push(invalid.functions[0].clone());
    let source = SourceFile::new("invalid-registration.kgr", "fn main() {}");
    let invalid = DefinitionMetadata::checked(definitions, invalid, &Default::default()).unwrap();
    let result = crate::analyze_source(&source, vec![Arc::new(invalid)]);
    assert!(matches!(
        result,
        Err(crate::analysis::error::AnalysisError::NativeApi(_))
    ));
}

#[test]
fn core_roles_use_complete_parsed_modules_with_attribute_trivia_and_docs() {
    let providers = catalog::shared().unwrap();
    let module = providers
        .iter()
        .find(|module| {
            module.records().identity.package.0 == "kagari-core"
                && module.records().identity.path == ["iter"]
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
        "/native-declarations",
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
    let id = module
        .records()
        .definition(DefinitionKind::Trait, "Iterator", module.definitions())
        .unwrap();
    assert!(lowered.registered_traits.contains_key(&id));
    let forged = source.text.replace("\"iterator\"", "\"iterable\"");
    source.text = forged;
    assert!(
        import_source(
            module,
            &providers,
            &source,
            "/native-declarations",
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
    let providers = catalog::shared().unwrap();
    let module = providers
        .iter()
        .find(|module| {
            module.records().identity.package.0 == "kagari-core"
                && module.records().identity.path == ["cmp"]
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
                "/native-declarations",
                Default::default(),
                &Default::default()
            )
            .is_err()
        );
    }
}

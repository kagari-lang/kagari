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

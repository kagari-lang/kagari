use super::*;
use crate::{
    native::{adoption::adopt, render::declaration_source},
    tests::native_fixture::module,
};
use kagari_common::identity::map::DefinitionContext;
use kagari_source::source::SourceFile;
use kagari_stdlib::{catalog as foundation_catalog, catalog};
use kagari_syntax::parser::ParseLimits;
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    ty::{GenericBound, NominalTy, Ty},
};

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
        ("{\n        self::default_read::<Self>(self)\n    }", ";"),
        ("core::hash::Hash", "core::cmp::Eq"),
        ("value0: T", "value0: i32"),
    ] {
        let mut source = original.clone();
        assert!(source.text.contains(from), "{from}: {}", source.text);
        source.text = source.text.replace(from, to);
        let error = PreparedRegistration::new(
            module.clone(),
            providers.clone().into(),
            Some(&source),
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
    let (definitions, mut definition) = module().as_ref().clone().into_parts();
    let template = definition
        .definition(DefinitionKind::Function, "default_read", &definitions)
        .unwrap();
    let interface = definition
        .definition(DefinitionKind::Trait, "NativeRead", &definitions)
        .unwrap();
    let requirements = ["read", "fixed", "read"].map(|name| NativeCallableRequirement {
        receiver: definition
            .functions
            .iter()
            .find(|function| function.name == "default_read")
            .unwrap()
            .generic_params[0]
            .as_type(),
        interface: NominalTy {
            declaration: interface,
            arguments: vec![],
            associated_types: Default::default(),
        },
        member: ModuleDecl::method_id(interface, name, &definitions).unwrap(),
        arguments: vec![],
    });
    let echo = definition
        .functions
        .iter_mut()
        .find(|function| function.name == "echo")
        .unwrap();
    let result = echo.generic_params[0].as_type();
    echo.return_type = Ty::Trait(requirements[0].interface.clone());
    echo.bounds.push(GenericBound {
        ty: result.clone(),
        constraints: vec![requirements[0].interface.clone()],
    });
    definition
        .concrete_results
        .insert(echo.generic_params[0].owner, result);
    definition
        .callable_requirements
        .insert(template, requirements.into());
    let module = Arc::new(
        DefinitionMetadata::checked(definitions, definition, &Default::default()).unwrap(),
    );
    let mut providers = catalog::shared().unwrap();
    providers.push(module);
    let providers = adopt(
        &providers,
        &DefinitionContext::new().unwrap(),
        &Default::default(),
    )
    .unwrap();
    let module = providers.last().unwrap();
    let mut source = declaration_source(module, &providers).unwrap();
    source.text = format!("// Tooling overlay.\n{}", source.text);
    for supplied in [None, Some(&source)] {
        // A source-less registration must not consume parser budgets at all.
        let limits = ParseLimits {
            max_nesting: if supplied.is_none() {
                0
            } else {
                ParseLimits::default().max_nesting
            },
            ..Default::default()
        };
        let prepared = PreparedRegistration::new(
            module.clone(),
            providers.clone().into(),
            supplied,
            "/native-declarations",
            limits,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(prepared.parsed().is_some(), supplied.is_some());
        let lowered = prepared.attach(&Default::default()).unwrap();
        assert_eq!(lowered.source_surface().is_some(), supplied.is_some());
        assert_eq!(lowered.module.traits.len(), 1);
        assert_eq!(lowered.module.traits[0].methods.len(), 2);
        assert!(
            lowered.module.traits[0]
                .methods
                .iter()
                .all(|method| method.has_default)
        );
        let natives = lowered
            .module
            .functions
            .iter()
            .filter(|function| matches!(function.implementation, FunctionImplementation::Native(_)))
            .collect::<Vec<_>>();
        assert_eq!(natives.len(), 4);
        for function in natives {
            let entry = module
                .records()
                .native_declarations(module.definitions())
                .unwrap()
                .into_iter()
                .find(|declaration| declaration.function.name == function.name)
                .unwrap()
                .declaration;
            {
                {
                    let FunctionImplementation::Native(contract) = &function.implementation else {
                        panic!("installed entry must own a Native implementation");
                    };
                    assert_eq!(
                        contract.callable_requirements.as_slice(),
                        module
                            .records()
                            .callable_requirements
                            .get(&entry)
                            .map(Vec::as_slice)
                            .unwrap_or_default()
                    );
                    assert_eq!(
                        contract.concrete_result.as_ref(),
                        module.records().concrete_results.get(&entry)
                    );
                    if function.name == "default_read" {
                        assert_eq!(contract.callable_requirements.len(), 3);
                        assert_ne!(
                            contract.callable_requirements[0],
                            contract.callable_requirements[1]
                        );
                        assert_eq!(
                            contract.callable_requirements[0],
                            contract.callable_requirements[2]
                        );
                    }
                    if function.name == "echo" {
                        assert!(matches!(
                            contract.concrete_result,
                            Some(Ty::Parameter { .. })
                        ));
                    }
                }
            }
        }
        for function in &lowered.module.functions {
            if function.implementation.script_body().is_some() {
                let FunctionImplementation::Script(body) = function.implementation else {
                    panic!("native default must retain its forwarding proof body");
                };
                let call = lowered.module.block(body).tail_expr.unwrap();
                let crate::hir::expr::ExprKind::Call {
                    args, type_args, ..
                } = &lowered.module.expr(call).kind
                else {
                    panic!("forwarding default must be a real checked call");
                };
                assert_eq!(args.len(), function.params.len());
                assert_eq!(type_args.as_ref().map_or(0, |arguments| arguments.len()), 1);
                let origin = lowered.module.expr(call).origin;
                assert_eq!(origin.source_span().is_some(), supplied.is_some());
                if supplied.is_none() {
                    assert!(matches!(
                        origin,
                        crate::hir::origin::NodeOrigin::Structured { .. }
                    ));
                }
                let registered = &module.records().traits[0]
                    .methods
                    .iter()
                    .find(|method| method.name == function.name)
                    .unwrap();
                assert_eq!(
                    lowered
                        .module
                        .traits
                        .iter()
                        .flat_map(|item| &item.methods)
                        .find(|method| method.function == function.id)
                        .unwrap()
                        .override_allowed,
                    registered.method_policy.override_allowed
                );
            }
        }
    }
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
    let prepared = PreparedRegistration::new(
        module.clone(),
        providers.clone().into(),
        Some(&source),
        "/native-declarations",
        Default::default(),
        &Default::default(),
    )
    .unwrap();
    let _parsed = prepared.parsed();
    let lowered = prepared.attach(&Default::default()).unwrap();

    let iterator = lowered
        .module
        .traits
        .iter()
        .find(|item| item.name == "Iterator")
        .unwrap();
    assert!(iterator.methods.iter().any(|method| method.name == "next"));
    let id = module
        .records()
        .definition(DefinitionKind::Trait, "Iterator", module.definitions())
        .unwrap();
    assert_eq!(
        module
            .definitions()
            .resolve(id)
            .unwrap()
            .last()
            .unwrap()
            .name,
        iterator.name
    );
    let forged = source.text.replace("\"iterator\"", "\"iterable\"");
    source.text = forged;
    assert!(
        PreparedRegistration::new(
            module.clone(),
            providers.clone().into(),
            Some(&source),
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
            PreparedRegistration::new(
                module.clone(),
                providers.clone().into(),
                Some(&source),
                "/native-declarations",
                Default::default(),
                &Default::default()
            )
            .is_err()
        );
    }
}

mod exact;
mod host;
mod reference;
mod script;

#[test]
fn native_trait_defaults_remain_native_without_script_bodies() {
    let (definitions, mut definition) = module().as_ref().clone().into_parts();
    let owner = definition
        .definition(DefinitionKind::Trait, "NativeRead", &definitions)
        .unwrap();
    let method = ModuleDecl::method_id(owner, "read", &definitions).unwrap();
    definition.traits[0].methods[0].implementation = CallableImplementation::Native(method);
    definition.script_bodies.remove(&method);
    let metadata = Arc::new(
        DefinitionMetadata::checked(definitions, definition, &Default::default()).unwrap(),
    );
    assert!(
        metadata
            .records()
            .native_declarations(metadata.definitions())
            .unwrap()
            .iter()
            .any(|declaration| declaration.declaration == method)
    );
    let mut providers = catalog::shared().unwrap();
    providers.push(metadata.clone());
    let source = declaration_source(&metadata, &providers).unwrap();
    for supplied in [None, Some(&source)] {
        let lowered = PreparedRegistration::new(
            metadata.clone(),
            providers.clone().into(),
            supplied,
            "/native",
            Default::default(),
            &Default::default(),
        )
        .unwrap()
        .attach(&Default::default())
        .unwrap();
        let method = lowered.module.traits[0]
            .methods
            .iter()
            .find(|method| method.name == "read")
            .unwrap();
        assert!(method.has_default);
        assert!(matches!(
            lowered.module.functions[method.function.index()].implementation,
            FunctionImplementation::Native(_)
        ));
        assert!(
            lowered.module.functions[method.function.index()]
                .implementation
                .script_body()
                .is_none()
        );
    }
}

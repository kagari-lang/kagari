//! Identify the forward conversion from its defining scope and resolved method.
use crate::{aggregates::traits::MethodSignature, types::TypeId};
use kagari_common::identity::{
    DefinitionKind,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    declaration::conversion::ConversionAdapter,
    language::{Protocol, binding},
};

pub(crate) fn try_from(definitions: &DefinitionTable) -> Option<DefinitionId> {
    let root = definitions.lookup_root(&Protocol::From.module())?;
    definitions.lookup_child(root, DefinitionKind::Trait, "TryFrom", 0)
}

/// Extract the member IDs from the checked return type. `valid_in` separately checks
/// the input binder, `Self`, the projection receiver/interface and the complete shape.
pub(crate) fn relationship(
    owner: DefinitionId,
    methods: &[MethodSignature],
    definitions: &DefinitionTable,
) -> Option<ConversionAdapter> {
    let [method] = methods else { return None };
    let TypeId::Enum(result) = &method.return_type else {
        return None;
    };
    let [_, TypeId::Projection { member, .. }] = result.arguments.as_slice() else {
        return None;
    };
    let [input] = method.params.as_slice() else {
        return None;
    };
    (method.name == "try_from"
        && method.default.is_none()
        && method.policy.override_allowed
        && input.name != "self"
        && definitions.lookup_child(owner, DefinitionKind::AssociatedType, "Error", 0)
            == Some(*member)
        && definitions.lookup(&binding::result_declaration()) == Some(result.declaration))
    .then_some(ConversionAdapter {
        method: method.id,
        error: *member,
        result: result.declaration,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        analysis::{AnalysisDatabase, core_source::packaged_modules},
        typeck::table::CallTarget,
        types::TypeId,
    };
    use kagari_common::identity::DefinitionKind;
    use kagari_source::{
        diagnostic::DiagnosticKind,
        source::SourceFile,
        source_database::{SourceDatabase, SourceLayer, SourceSnapshot},
    };
    use kagari_types::language::{Protocol, binding};
    use std::sync::Arc;

    #[test]
    fn authored_try_from_rejects_a_different_result_constructor() {
        let cancel = Default::default();
        let mut convert = packaged_modules()
            .iter()
            .find(|m| *m.source.module_identity() == Protocol::From.module())
            .unwrap()
            .clone();
        // Both the forward method and its blanket still agree on a binary enum.
        // Agreement alone must not authorize that enum as the conversion result.
        let mut sources = SourceDatabase::default();
        let uri = convert.source.name();
        sources
            .bind_module(uri, convert.source.module_identity().clone())
            .unwrap();
        let id = sources
            .set(
                uri,
                convert.source.text().replace(
                    "use core::result::Result;",
                    "use core::ops::ControlFlow as Result;",
                ),
                SourceLayer::Base,
            )
            .unwrap();
        convert.source = sources.snapshot().file(id).unwrap().clone();
        let mut db = AnalysisDatabase::new("/app").unwrap();
        db.replace_core_fixture(convert);
        let snapshot = db.snapshot(SourceSnapshot::default(), &cancel).unwrap();
        let file = snapshot.file(id).unwrap();
        let definitions = file.definitions();
        let signature = file
            .result()
            .facts()
            .aggregates
            .trait_(&super::try_from(definitions).unwrap())
            .unwrap();
        assert!(
            matches!(&signature.methods[0].return_type, TypeId::Enum(result) if Some(result.declaration) == definitions.lookup(&binding::control_flow_declaration()))
        );
        let diagnostics = file.result().diagnostics();
        assert!(diagnostics.iter().any(|d| matches!(&d.kind, DiagnosticKind::InvalidLanguageRole { reason, .. } if reason == "invalid static conversion relationship")), "{diagnostics:?}");
        assert!(snapshot.check_program(id, &cancel).is_err());
    }

    #[test]
    fn authored_try_from_selects_only_the_core_forward_relationship() {
        let cancel = Default::default();
        let modules = packaged_modules();
        let convert = modules
            .iter()
            .find(|m| *m.source.module_identity() == Protocol::From.module())
            .unwrap();
        // Check the very same declarations under an ordinary owner. Only the
        // canonical module may claim From's lang attribute.
        let ordinary = Arc::new(SourceFile::new(
            "/app/convert.kgr",
            convert.source.text().replace("#[lang = \"from\"]", ""),
        ));
        let mut db = AnalysisDatabase::new("/app").unwrap();

        let snapshot = db
            .snapshot(SourceSnapshot::single_file(ordinary.clone()), &cancel)
            .unwrap();
        snapshot.check_program(ordinary.id(), &cancel).unwrap();
        for (file, canonical) in [(convert.source.id(), true), (ordinary.id(), false)] {
            let file = snapshot.file(file).unwrap();
            let definitions = file.definitions();
            let facts = file.result().facts();
            let root = definitions
                .lookup_root(file.source().module_identity())
                .unwrap();
            let owner = definitions
                .lookup_child(root, DefinitionKind::Trait, "TryFrom", 0)
                .unwrap();
            let method = definitions
                .lookup_child(owner, DefinitionKind::Method, "try_from", 0)
                .unwrap();
            let adapter = &facts.aggregates.trait_(&owner).unwrap().conversion_adapter;
            if canonical {
                let adapter = adapter.as_ref().unwrap();
                assert_eq!(adapter.method, method);
                assert_eq!(
                    Some(adapter.error),
                    definitions.lookup_child(owner, DefinitionKind::AssociatedType, "Error", 0)
                );
                assert_eq!(
                    Some(adapter.result),
                    definitions.lookup(&binding::result_declaration())
                );
            } else {
                assert!(adapter.is_none());
            }
            let hir = &facts.lowered.module;
            let forwarder = hir
                .functions
                .iter()
                .find(|f| f.name == "try_into" && f.implementation.script_body().is_some())
                .unwrap();
            let call = hir
                .block(forwarder.implementation.script_body().unwrap())
                .tail_expr
                .unwrap();
            let resolution = facts.typed.type_table().call_resolution(call).unwrap();
            assert!(
                matches!(&resolution.target, CallTarget::TraitMethod { method: target, interface } if *target == method && interface.declaration == owner)
            );
        }
    }
}

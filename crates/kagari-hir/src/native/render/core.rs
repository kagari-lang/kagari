//! Coordinates and documentation for the handwritten portion of the foundation.
use crate::{
    language::source::trait_source,
    lower::context::{syntax_span, token_span},
    native::render::{DeclarationSource, NativeDeclarationSite},
};
use kagari_contract::{
    declaration::ModuleDecl,
    language::{Protocol, role::LangRole},
};
use kagari_syntax::{ast::item::Item, parser::parse};
use {
    kagari_common::identity::{DefinitionKind, associated_type_id},
    kagari_source::source::SourceFile,
};

pub(super) fn core_text(role: LangRole) -> String {
    format!("{}\n", trait_source(role))
}

pub(super) fn record_sites(source: &mut DeclarationSource, module: &ModuleDecl) {
    let parsed = parse(&SourceFile::new(&source.uri, &source.text));
    for item in parsed.syntax().items() {
        let Item::TraitDef(item) = item else { continue };
        let owner = module.definition(DefinitionKind::Trait, &item.name_text().unwrap());
        if Protocol::from_id(&owner)
            .and_then(LangRole::from_protocol)
            .is_none()
        {
            continue;
        }
        let mut site = |id, span, name| {
            source.sites.insert(
                id,
                NativeDeclarationSite {
                    span,
                    name_span: token_span(&name),
                    generics: vec![],
                    parameters: vec![],
                    bounds: vec![],
                },
            );
        };
        site(owner.clone(), syntax_span(&item), item.name().unwrap());
        for member in item.associated_types() {
            site(
                associated_type_id(&owner, &member.name_text().unwrap()),
                syntax_span(&member),
                member.name().unwrap(),
            );
        }
        for method in item.methods() {
            site(
                ModuleDecl::method_id(&owner, &method.name_text().unwrap()),
                syntax_span(&method),
                method.name().unwrap(),
            );
        }
    }
}

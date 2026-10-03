//! Coordinates and documentation for the handwritten portion of the foundation.
use crate::{
    language::source::trait_source,
    lower::context::{syntax_span, token_span},
    native::render::{DeclarationSource, NativeDeclarationSite},
};
use kagari_common::{
    identity::{DefinitionKind, DefinitionPath, associated_type_id},
    source::SourceFile,
};
use kagari_contract::{
    declaration::ModuleDecl,
    language::{Protocol, role::LangRole},
};
use kagari_syntax::{ast::item::Item, parser::parse};

pub(super) fn core_text(module: &ModuleDecl, owner: &DefinitionPath, role: LangRole) -> String {
    let mut text = String::new();
    for line in trait_source(role).lines() {
        let trimmed = line.trim_start();
        let id = if trimmed.starts_with("#[lang") {
            Some(owner.clone())
        } else if let Some(method) = trimmed.strip_prefix("fn ") {
            Some(ModuleDecl::method_id(
                owner,
                method.split('(').next().unwrap(),
            ))
        } else {
            trimmed
                .strip_prefix("type ")
                .map(|member| associated_type_id(owner, member.split([';', ':']).next().unwrap()))
        };
        if let Some(docs) = id.as_ref().and_then(|id| module.documentation.get(id)) {
            for doc in docs.lines() {
                if !trimmed.starts_with("#[lang") {
                    text.push_str("    ");
                }
                text.push_str("/// ");
                text.push_str(doc);
                text.push('\n');
            }
        }
        text.push_str(line);
        text.push('\n');
    }
    text
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

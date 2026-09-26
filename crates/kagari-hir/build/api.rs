use super::*;

pub fn name_range(name: &ast::Name) -> (usize, usize) {
    name.syntax()
        .children_with_tokens()
        .filter_map(|node| node.into_token())
        .find(|token| token.kind() == kagari_syntax::kind::SyntaxKind::Ident)
        .map(|token| {
            let range = token.text_range();
            (usize::from(range.start()), usize::from(range.end()))
        })
        .expect("declaration identifier")
}

pub fn item(
    node: &impl AstNode,
    name: ast::Name,
    module: &str,
    uri: &str,
    text: &str,
    path: &[(&str, String)],
) -> String {
    let range = name_range(&name);
    let (start, end) = range;
    let documentation = node.documentation(text);
    assert!(!documentation.is_empty(), "undocumented {module}::{path:?}");
    let signature = node.syntax().text().to_string();
    let path = path
        .iter()
        .map(|(kind, name)| format!("(kagari_common::identity::DefinitionKind::{kind},{name:?})"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "ApiItem{{module:{module:?},uri:{uri:?},path:&[{path}],start:{start},end:{end},documentation:{documentation:?},signature:{signature:?}}}"
    )
}

fn bound(node: ast::TraitRef) -> String {
    let name = node.path_text().unwrap();
    let args = node
        .generic_args()
        .into_iter()
        .flat_map(|a| a.args().collect::<Vec<_>>())
        .map(ty)
        .collect::<Vec<_>>()
        .join(",");
    let bindings = node
        .generic_args()
        .into_iter()
        .flat_map(|a| a.bindings().collect::<Vec<_>>())
        .map(|b| format!("({:?},{})", b.name_text().unwrap(), ty(b.ty().unwrap())))
        .collect::<Vec<_>>()
        .join(",");
    format!("ApiBound{{name:{name:?},args:&[{args}],bindings:&[{bindings}]}}")
}
fn bounds(list: Option<ast::TraitBoundList>) -> String {
    list.into_iter()
        .flat_map(|l| l.bounds().collect::<Vec<_>>())
        .map(bound)
        .collect::<Vec<_>>()
        .join(",")
}

pub fn declarations(
    parsed: &kagari_syntax::Parse,
    module: &str,
    uri: &str,
    text: &str,
    items: &mut String,
    traits: &mut String,
    enums: &mut String,
    constructors: &mut String,
) {
    for node in parsed.syntax().syntax().children() {
        if let Some(def) = ast::TraitDef::cast(node.clone()) {
            let name = def.name_text().unwrap();
            let path = [("Trait", name.clone())];
            let declaration = item(&def, def.name().unwrap(), module, uri, text, &path);
            writeln!(items, "{declaration},").unwrap();
            let generics = def
                .generic_params()
                .into_iter()
                .flat_map(|p| p.params().collect::<Vec<_>>())
                .map(|p| p.name_text().unwrap())
                .collect::<Vec<_>>();
            let supers = bounds(def.supertraits());
            let mut associated = String::new();
            let mut methods = String::new();
            for member in def.associated_types() {
                let path = [
                    ("Trait", name.clone()),
                    ("AssociatedType", member.name_text().unwrap()),
                ];
                let declaration = item(&member, member.name().unwrap(), module, uri, text, &path);
                writeln!(items, "{declaration},").unwrap();
                writeln!(
                    associated,
                    "ApiAssociatedType{{item:{declaration},bounds:&[{}]}},",
                    bounds(member.bounds())
                )
                .unwrap();
            }
            for method in def.methods() {
                assert!(
                    method.body().is_none(),
                    "standard protocols have no source default body"
                );
                assert!(
                    method.generic_params().is_none(),
                    "unexpected generic standard method"
                );
                let path = [
                    ("Trait", name.clone()),
                    ("Method", method.name_text().unwrap()),
                ];
                let declaration = item(&method, method.name().unwrap(), module, uri, text, &path);
                writeln!(items, "{declaration},").unwrap();
                let params = method
                    .param_list()
                    .unwrap()
                    .params()
                    .map(|p| {
                        format!(
                            "ApiParameter{{name:{:?},ty:{}}}",
                            p.name_text().unwrap(),
                            p.ty()
                                .map(ty)
                                .unwrap_or("ApiType::Named(\"Self\",&[])".into())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let result = method
                    .return_type()
                    .map(ty)
                    .unwrap_or("ApiType::Tuple(&[])".into());
                writeln!(
                    methods,
                    "ApiMethod{{item:{declaration},params:&[{params}],result:{result}}},"
                )
                .unwrap();
            }
            writeln!(traits,"ApiTrait{{item:{declaration},generics:&{generics:?},supertraits:&[{supers}],associated_types:&[{associated}],methods:&[{methods}]}},").unwrap();
        } else if let Some(def) = ast::EnumDef::cast(node.clone()) {
            let name = def.name_text().unwrap();
            let binding = attribute(&def, "builtin_enum").expect("native enum binding");
            assert_eq!(binding, name, "native enum declaration name");
            writeln!(
                items,
                "{},",
                item(
                    &def,
                    def.name().unwrap(),
                    module,
                    uri,
                    text,
                    &[("Enum", name.clone())]
                )
            )
            .unwrap();
            let arity = def.generic_params().map_or(0, |p| p.params().count());
            let mut variants = String::new();
            for variant in def.variant_list().unwrap().variants() {
                let variant_name = variant.name_text().unwrap();
                let payload_arity = variant.payload_types().map_or(0, |p| p.types().count());
                writeln!(
                    variants,
                    "StandardVariantSpec{{name:{variant_name:?},payload_arity:{payload_arity}}},"
                )
                .unwrap();
                writeln!(
                    items,
                    "{},",
                    item(
                        &variant,
                        variant.name().unwrap(),
                        module,
                        uri,
                        text,
                        &[("Enum", name.clone()), ("Variant", variant_name)]
                    )
                )
                .unwrap();
            }
            writeln!(enums,"StandardEnumSpec{{kind:StandardEnum::{binding},name:{name:?},arity:{arity},variants:&[{variants}]}},").unwrap();
            if arity > 0 {
                writeln!(constructors,"StandardTypeConstructorSpec{{kind:StandardTypeConstructor::{binding},name:{name:?},arity:{arity},heap_backed:true,const_safe:false}},").unwrap();
            }
        } else if let Some(def) = ast::AssociatedType::cast(node) {
            let name = def.name_text().unwrap();
            let binding = attribute(&def, "builtin_type").expect("native type binding");
            assert_eq!(binding, name, "native type declaration name");
            writeln!(
                items,
                "{},",
                item(
                    &def,
                    def.name().unwrap(),
                    module,
                    uri,
                    text,
                    &[("AssociatedType", name.clone())]
                )
            )
            .unwrap();
            let arity = def.generic_params().map_or(0, |p| p.params().count());
            if matches!(binding.as_str(), "Map" | "Set" | "Cursor") {
                writeln!(constructors,"StandardTypeConstructorSpec{{kind:StandardTypeConstructor::{binding},name:{name:?},arity:{arity},heap_backed:true,const_safe:false}},").unwrap();
            }
        }
    }
}

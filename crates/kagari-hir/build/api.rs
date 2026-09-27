use super::*;

/// Bind native trait declarations to the engine's sealed implementation contracts.
pub fn native_implementation(
    def: &ast::ImplBlock,
    module: &str,
    uri: &str,
    text: &str,
    items: &mut String,
    implementations: &mut String,
) {
    let interface = def.trait_ref().unwrap();
    let target = def.target_type().unwrap();
    let generics = def.generic_params().unwrap().params().collect::<Vec<_>>();
    assert_eq!(generics.len(), 1, "native iterator generic arity");
    let parameter = generics[0].name_text().unwrap();
    assert!(generics[0].bounds().is_none());
    assert!(def.where_clause().is_none());
    assert!(def.associated_consts().next().is_none());
    assert_eq!(module, "iter");
    assert_eq!(interface.path_text().as_deref(), Some("Iterator"));
    assert!(interface.generic_args().is_none());
    let parameter_type = format!("ApiType::Named({parameter:?}, &[])");
    assert_eq!(
        ty(target.clone()),
        format!("ApiType::Named(\"Iter\", &[{parameter_type}])")
    );

    let associated = def.associated_types().collect::<Vec<_>>();
    assert_eq!(associated.len(), 1, "native iterator associated types");
    let member = &associated[0];
    assert_eq!(member.name_text().as_deref(), Some("Item"));
    assert!(member.generic_params().is_none() && member.bounds().is_none());
    assert!(member.where_clause().is_none());
    assert_eq!(ty(member.ty().unwrap()), parameter_type);
    let owner = "Iterator for Iter".to_owned();
    let member_item = item(
        member,
        member.name().unwrap(),
        module,
        uri,
        text,
        &[("Impl", owner.clone()), ("AssociatedType", "Item".into())],
    );
    writeln!(items, "{member_item},").unwrap();

    let methods = def.methods().collect::<Vec<_>>();
    assert_eq!(methods.len(), 1, "native iterator required methods");
    let method = &methods[0];
    assert_eq!(method.name_text().as_deref(), Some("next"));
    assert_eq!(attribute(method, "intrinsic").as_deref(), Some("IterNext"));
    assert_eq!(method.visibility(), ast::Visibility::Private);
    assert!(method.body().is_none() && method.generic_params().is_none());
    assert!(method.where_clause().is_none());
    let params = method.param_list().unwrap().params().collect::<Vec<_>>();
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name_text().as_deref(), Some("self"));
    assert!(params[0].ty().is_none());
    let result = ty(method.return_type().unwrap());
    assert_eq!(
        result,
        format!("ApiType::Named(\"Option\", &[{parameter_type}])")
    );
    let method_item = item(
        method,
        method.name().unwrap(),
        module,
        uri,
        text,
        &[("Impl", owner), ("Method", "next".into())],
    );
    writeln!(items, "{method_item},").unwrap();
    let generics = vec![parameter];
    let target = ty(target);
    writeln!(implementations,
        "super::declarations::ApiImplementation{{interface:\"Iterator\",generics:&{generics:?},target:{target},associated_types:&[({member_item},{parameter_type})],methods:&[ApiMethod{{item:{method_item},iterator:None,generics:&[],bounds:&[],params:&[ApiParameter{{name:\"self\",ty:{target}}}],result:{result}}}]}},"
    ).unwrap();
}

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

pub fn bound(node: ast::TraitRef) -> String {
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

/// Native enum layout is an engine ABI, not an editable declaration convention.
fn validate_native_enum(def: &ast::EnumDef, binding: &str) {
    let (arity, layout): (usize, &[(&str, Option<usize>)]) = match binding {
        "Option" => (1, &[("Some", Some(0)), ("None", None)]),
        "Result" => (2, &[("Ok", Some(0)), ("Err", Some(1))]),
        "Ordering" => (0, &[("Less", None), ("Equal", None), ("Greater", None)]),
        _ => panic!("unknown native enum binding {binding}"),
    };
    let parameters = def
        .generic_params()
        .into_iter()
        .flat_map(|p| p.params().collect::<Vec<_>>())
        .map(|p| p.name_text().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(parameters.len(), arity, "{binding} ABI generic arity");
    let variants = def.variant_list().unwrap().variants().collect::<Vec<_>>();
    assert_eq!(variants.len(), layout.len(), "{binding} ABI variants");
    for (variant, (name, payload)) in variants.iter().zip(layout) {
        assert_eq!(
            variant.name_text().as_deref(),
            Some(*name),
            "{binding} ABI discriminant"
        );
        let types = variant
            .payload_types()
            .into_iter()
            .flat_map(|p| p.types().collect::<Vec<_>>())
            .collect::<Vec<_>>();
        assert_eq!(
            types.len(),
            usize::from(payload.is_some()),
            "{binding}::{name} ABI payload arity"
        );
        if let Some(index) = payload {
            assert_eq!(
                types[0].name_text().as_deref(),
                Some(parameters[*index].as_str()),
                "{binding}::{name} ABI payload type"
            );
            assert!(
                types[0].generic_args().is_none(),
                "native payload is a generic parameter"
            );
        }
    }
}

pub fn declarations(
    parsed: &kagari_syntax::Parse,
    module: &str,
    uri: &str,
    text: &str,
    outputs: (&mut String, &mut String, &mut String, &mut String),
) {
    let (items, traits, enums, constructors) = outputs;
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
                let iterator = attribute(&method, "intrinsic")
                    .map(|binding| {
                        assert_eq!(name, "Iterator", "native defaults belong to Iterator");
                        let operation = binding
                            .strip_prefix("Iterator")
                            .expect("iterator operation");
                        format!("Some(super::declarations::IteratorMethod::{operation})")
                    })
                    .unwrap_or("None".into());
                assert!(
                    method.body().is_none(),
                    "standard protocols have no source default body"
                );
                let generics = method
                    .generic_params()
                    .into_iter()
                    .flat_map(|p| p.params().collect::<Vec<_>>())
                    .map(|p| {
                        let name=p.name_text().unwrap(); let key=format!("@bound:{name}");
                        format!("super::declarations::ApiGeneric{{name:{name:?}, bounds:&[{}],projection_key:{key:?}}}", bounds(p.bounds()))
                    })
                    .collect::<Vec<_>>()
                    .join(",");
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
                let predicates = method
                    .where_clause()
                    .into_iter()
                    .flat_map(|w| w.predicates().collect::<Vec<_>>())
                    .map(|p| {
                        format!(
                            "({}, &[{}])",
                            ty(p.target_type().unwrap()),
                            bounds(p.bounds())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                writeln!(
                    methods,
                    "ApiMethod{{item:{declaration},iterator:{iterator},generics:&[{generics}],bounds:&[{predicates}],params:&[{params}],result:{result}}},"
                )
                .unwrap();
            }
            writeln!(traits,"ApiTrait{{item:{declaration},generics:&{generics:?},supertraits:&[{supers}],associated_types:&[{associated}],methods:&[{methods}]}},").unwrap();
        } else if let Some(def) = ast::EnumDef::cast(node.clone()) {
            let name = def.name_text().unwrap();
            let binding = attribute(&def, "builtin_enum").expect("native enum binding");
            assert_eq!(binding, name, "native enum declaration name");
            validate_native_enum(&def, &binding);
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
            if matches!(
                binding.as_str(),
                "Array" | "MutableArray" | "Map" | "MutableMap" | "Set" | "MutableSet" | "Iter"
            ) {
                writeln!(constructors,"StandardTypeConstructorSpec{{kind:StandardTypeConstructor::{binding},name:{name:?},arity:{arity},heap_backed:true,const_safe:false}},").unwrap();
            }
        }
    }
}

/// Shared AST facade for native free functions and associated declarations.
#[derive(Clone)]
pub struct NativeFunction(kagari_syntax::syntax_node::SyntaxNode);
impl AstNode for NativeFunction {
    fn can_cast(kind: kagari_syntax::kind::SyntaxKind) -> bool {
        ast::FnDef::can_cast(kind) || ast::MethodDef::can_cast(kind)
    }
    fn cast(node: kagari_syntax::syntax_node::SyntaxNode) -> Option<Self> {
        Self::can_cast(node.kind()).then_some(Self(node))
    }
    fn syntax(&self) -> &kagari_syntax::syntax_node::SyntaxNode {
        &self.0
    }
}
impl NativeFunction {
    pub fn name(&self) -> Option<ast::Name> {
        self.0.children().find_map(ast::Name::cast)
    }
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|n| n.text())
    }
    pub fn generic_params(&self) -> Option<ast::GenericParamList> {
        self.0.children().find_map(ast::GenericParamList::cast)
    }
    pub fn param_list(&self) -> Option<ast::ParamList> {
        self.0.children().find_map(ast::ParamList::cast)
    }
    pub fn return_type(&self) -> Option<ast::TypeRef> {
        self.0.children().find_map(ast::TypeRef::cast)
    }
    pub fn body(&self) -> Option<ast::BlockExpr> {
        self.0.children().find_map(ast::BlockExpr::cast)
    }
    pub fn visibility(&self) -> ast::Visibility {
        if let Some(f) = ast::FnDef::cast(self.0.clone()) {
            f.visibility()
        } else {
            ast::MethodDef::cast(self.0.clone()).unwrap().visibility()
        }
    }
}

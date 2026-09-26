#[path = "build/api.rs"]
mod api;
use kagari_common::{SourceFile, cancellation::CancellationToken};
use kagari_syntax::{
    ast::{self, AstNode},
    parser::{ParseLimits, parse_declarations},
};
use std::{collections::BTreeSet, fmt::Write, path::PathBuf};

fn ty(node: ast::TypeRef) -> String {
    if let Some(array) = node.array_type() {
        return format!("ApiType::Array(&{})", ty(array.element_type().unwrap()));
    }
    if let Some(tuple) = node.tuple_type() {
        return format!(
            "ApiType::Tuple(&[{}])",
            tuple.element_types().map(ty).collect::<Vec<_>>().join(",")
        );
    }
    if let Some(function) = node.function_type() {
        return format!(
            "ApiType::Function(&[{}], &{})",
            function.params().map(ty).collect::<Vec<_>>().join(","),
            function
                .result()
                .map(ty)
                .unwrap_or("ApiType::Tuple(&[])".into())
        );
    }
    if let Some(grouped) = node.grouped_type() {
        return ty(grouped);
    }
    let name = node.name_text().expect("named declaration type");
    let args = node
        .generic_args()
        .into_iter()
        .flat_map(|args| args.args().collect::<Vec<_>>())
        .map(ty)
        .collect::<Vec<_>>()
        .join(",");
    format!("ApiType::Named({name:?}, &[{args}])")
}

fn attribute(node: &impl AstNode, name: &str) -> Option<String> {
    let attrs = node
        .syntax()
        .children()
        .filter_map(ast::Attribute::cast)
        .filter(|a| a.path().and_then(|p| p.text()).as_deref() == Some(name))
        .collect::<Vec<_>>();
    assert!(attrs.len() <= 1, "duplicate {name} attribute");
    attrs.first().map(|attr| {
        let args = attr.args().unwrap().arguments().collect::<Vec<_>>();
        assert_eq!(args.len(), 1, "{name} takes one identifier");
        args[0].value().unwrap().path().unwrap().text().unwrap()
    })
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../stdlib");
    let mut items = String::new();
    let mut traits = String::new();
    let mut enums = String::new();
    let mut constructors = String::new();
    let mut functions = String::new();
    let mut methods = String::new();
    let mut sources = String::new();
    let mut bindings = BTreeSet::new();
    let mut method_bindings = BTreeSet::new();
    for module in [
        "Array", "Map", "Set", "String", "Option", "Result", "Iter", "Math", "Debug", "Cmp",
        "Hash", "Fmt", "Ops", "Convert",
    ] {
        let file = format!("{}.kgr", module.to_lowercase());
        let path = root.join(&file);
        println!("cargo:rerun-if-changed={}", path.display());
        let text = std::fs::read_to_string(&path).expect("standard declaration source");
        let uri = format!("kagari://std/{file}");
        writeln!(sources, "({uri:?}, {text:?}),").unwrap();
        let source = SourceFile::new(&uri, &text);
        let parsed = parse_declarations(
            &source,
            ParseLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap();
        assert!(
            parsed.diagnostics().is_empty(),
            "{file}: {:?}",
            parsed.diagnostics()
        );
        api::declarations(
            &parsed,
            &module.to_lowercase(),
            &uri,
            &text,
            (&mut items, &mut traits, &mut enums, &mut constructors),
        );
        let mut exports = BTreeSet::new();
        for item in parsed.syntax().items() {
            let ast::Item::FnDef(function) = item else {
                continue;
            };
            let name = function.name_text().unwrap();
            writeln!(
                items,
                "{},",
                api::item(
                    &function,
                    function.name().unwrap(),
                    &module.to_lowercase(),
                    &uri,
                    &text,
                    &[("Function", name.clone())]
                )
            )
            .unwrap();
            assert!(
                exports.insert(name.clone()),
                "duplicate export {module}::{name}"
            );
            assert!(
                function.body().is_none(),
                "native declarations cannot have bodies"
            );
            assert_eq!(function.visibility(), ast::Visibility::Public);
            let intrinsic = attribute(&function, "intrinsic").expect("missing intrinsic binding");
            assert!(
                bindings.insert(intrinsic.clone()),
                "duplicate intrinsic binding {intrinsic}"
            );
            let mut generics = Vec::new();
            let mut constraints = Vec::new();
            for param in function
                .generic_params()
                .into_iter()
                .flat_map(|list| list.params().collect::<Vec<_>>())
            {
                let param_name = param.name_text().unwrap();
                assert!(
                    !generics.contains(&param_name),
                    "duplicate generic parameter"
                );
                let bounds = param
                    .bounds()
                    .into_iter()
                    .flat_map(|b| b.bounds().collect::<Vec<_>>())
                    .map(|b| b.path().unwrap().text().unwrap())
                    .collect::<Vec<_>>();
                let constraint = match bounds
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    [] => None,
                    ["Eq", "Hash"] => Some("HashKey"),
                    ["PartialEq"] => Some("Comparable"),
                    ["Iterable"] => Some("Iterable"),
                    ["OrderedNumber"] => Some("OrderedNumber"),
                    ["SignedNumber"] => Some("SignedNumber"),
                    _ => panic!("unsupported standard constraint {bounds:?}"),
                };
                if let Some(c) = constraint {
                    constraints.push(format!("StandardConstraintSpec{{param:{param_name:?},constraint:StandardTypeConstraint::{c}}}"));
                }
                generics.push(param_name);
            }
            let params = function.param_list().unwrap().params().collect::<Vec<_>>();
            let arity = params.len();
            let parameter_types = params
                .iter()
                .map(|p| {
                    format!(
                        "ApiParameter{{name:{:?},ty:{}}}",
                        p.name_text().unwrap(),
                        ty(p.ty().unwrap())
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let output = function
                .return_type()
                .map(ty)
                .unwrap_or("ApiType::Tuple(&[])".into());
            let signature = function.syntax().text().to_string();
            let signature = &signature[signature.find("pub fn").unwrap()..];
            let doc = function.documentation(&text);
            assert!(!doc.is_empty(), "undocumented {name}");
            let range = api::name_range(&function.name().unwrap());
            let (start, end) = range;
            let generics = format!("&{:?}", generics);
            let constraints = format!("&[{}]", constraints.join(","));
            let qualified_name = format!("std::{}::{name}", module.to_lowercase());
            writeln!(functions,"StandardFunctionSpec{{module:StandardModule::{module},name:{name:?},intrinsic:StandardIntrinsic::{intrinsic},type_params:{generics},arity:{arity},constraints:{constraints},api:&ApiFunction{{qualified_name:{qualified_name:?},uri:{uri:?},start:{start},end:{end},documentation:{doc:?},signature:{signature:?},params:&[{parameter_types}],result:{output}}}}},").unwrap();
            if let Some(method) = attribute(&function, "method") {
                assert!(arity > 0, "method needs a receiver");
                let receiver = if module == "Iter" { "Iterable" } else { module };
                assert!(
                    method_bindings.insert((receiver.to_owned(), method.clone())),
                    "duplicate method binding"
                );
                writeln!(methods,"StandardMethodSpec{{receiver:StandardMethodReceiver::{receiver},name:{method:?},intrinsic:StandardIntrinsic::{intrinsic},type_params:{generics},arity:{},constraints:{constraints}}},",arity-1).unwrap();
            }
        }
    }
    let out = format!(
        "pub const STANDARD_ITEMS:&[ApiItem]=&[{items}];\npub const STANDARD_TRAITS:&[ApiTrait]=&[{traits}];\nconst STANDARD_ENUMS:&[StandardEnumSpec]=&[{enums}];\nconst STANDARD_TYPE_CONSTRUCTORS:&[StandardTypeConstructorSpec]=&[{constructors}];\nconst STANDARD_FUNCTIONS:&[StandardFunctionSpec]=&[{functions}];\nconst STANDARD_METHODS:&[StandardMethodSpec]=&[{methods}];\npub const STANDARD_SOURCES:&[(&str,&str)]=&[{sources}];"
    );
    std::fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("standard_api.rs"),
        out,
    )
    .unwrap();
}

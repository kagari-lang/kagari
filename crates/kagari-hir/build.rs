#[path = "build/api.rs"]
mod api;
#[path = "build/implementations.rs"]
mod implementations;
use kagari_common::{SourceFile, cancellation::CancellationToken};
use kagari_syntax::{
    ast::{self, AstNode},
    parser::{ParseLimits, parse_declarations},
};
use std::{collections::BTreeSet, fmt::Write, path::PathBuf};

fn ty(node: ast::TypeRef) -> String {
    if let Some(projection) = node.qualified_type() {
        assert!(
            projection.generic_args().is_none(),
            "native GAT projection is not declared yet"
        );
        return format!(
            "ApiType::Projection(&{}, &{}, {:?})",
            ty(projection.receiver().unwrap()),
            api::bound(projection.trait_ref().unwrap()),
            projection.member().unwrap().text().unwrap()
        );
    }
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
    let mut implementations = String::new();
    let mut functions = String::new();
    let mut methods = String::new();
    let mut sources = String::new();
    let mut bindings = BTreeSet::new();
    let mut method_bindings = BTreeSet::new();
    let mut implementation_bindings = BTreeSet::new();
    for module in [
        "Array", "Map", "Set", "String", "Option", "Result", "Iter", "Math", "Debug", "Cmp",
        "Hash", "Fmt", "Ops", "Convert", "Numeric",
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
        let mut native_functions = Vec::new();
        for item in parsed.syntax().items() {
            if let Some(function) = api::NativeFunction::cast(item.syntax().clone()) {
                native_functions.push((None, function));
            } else if let ast::Item::ImplBlock(implementation) = item {
                if implementation.trait_ref().is_some() {
                    assert!(
                        implementation_bindings.insert((
                            implementation.trait_ref().unwrap().path_text().unwrap(),
                            ty(implementation.target_type().unwrap()),
                        )),
                        "duplicate native trait implementation"
                    );
                    api::native_implementation(
                        &implementation,
                        &module.to_lowercase(),
                        &uri,
                        &text,
                        &mut items,
                        &mut implementations,
                    );
                    continue;
                }
                for method in implementation.methods() {
                    native_functions.push((
                        Some(implementation.clone()),
                        api::NativeFunction::cast(method.syntax().clone()).unwrap(),
                    ));
                }
            }
        }
        for (implementation, function) in native_functions {
            assert!(
                attribute(&function, "method").is_none(),
                "declare methods in impl blocks"
            );
            let target = implementation.as_ref().and_then(|i| i.target_type());
            let owner = target.as_ref().and_then(|t| t.name_text());
            let self_type = target.map(ty);
            let name = function.name_text().unwrap();
            let path = if let Some(owner) = &owner {
                vec![("AssociatedType", owner.clone()), ("Method", name.clone())]
            } else {
                vec![("Function", name.clone())]
            };
            let export = owner
                .as_ref()
                .map_or_else(|| name.clone(), |owner| format!("{owner}::{name}"));
            writeln!(
                items,
                "{},",
                api::item(
                    &function,
                    function.name().unwrap(),
                    &module.to_lowercase(),
                    &uri,
                    &text,
                    &path
                )
            )
            .unwrap();
            assert!(
                exports.insert(export.clone()),
                "duplicate export {module}::{name}"
            );
            assert!(
                function.body().is_none(),
                "native declarations cannot have bodies"
            );
            assert_eq!(function.visibility(), ast::Visibility::Public);
            let intrinsic = if attribute(&function, "parse_radix").is_some() {
                let owner = owner.as_ref().expect("radix parser owner");
                let builtin = match owner.as_str() {
                    "isize" => "ISize".to_owned(),
                    "usize" => "USize".to_owned(),
                    _ => {
                        let mut chars = owner.chars();
                        chars.next().unwrap().to_uppercase().to_string() + chars.as_str()
                    }
                };
                format!("ParseRadix(BuiltinType::{builtin})")
            } else if let Some(operation) = attribute(&function, "numeric") {
                let owner = owner.as_ref().expect("numeric receiver");
                let builtin = match owner.as_str() {
                    "isize" => "ISize".to_owned(),
                    "usize" => "USize".to_owned(),
                    _ => {
                        let mut chars = owner.chars();
                        chars.next().unwrap().to_uppercase().to_string() + chars.as_str()
                    }
                };
                format!(
                    "Integer(kagari_common::integer::IntegerMethod::{operation}, BuiltinType::{builtin})"
                )
            } else {
                attribute(&function, "intrinsic").expect("missing intrinsic binding")
            };
            assert!(
                bindings.insert(intrinsic.clone()),
                "duplicate intrinsic binding {intrinsic}"
            );
            let mut generics = Vec::new();
            let mut constraints = Vec::new();
            let mut predicates = Vec::new();
            for param in implementation
                .as_ref()
                .and_then(|i| i.generic_params())
                .into_iter()
                .chain(function.generic_params())
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
                    ["RangeBounds"] | ["FromStr"] => {
                        let bounds = api::bounds(param.bounds());
                        predicates.push(format!(
                            "(ApiType::Named({param_name:?}, &[]), &[{bounds}])"
                        ));
                        None
                    }
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
                        p.ty()
                            .map(ty)
                            .unwrap_or_else(|| {
                                assert_eq!(p.name_text().as_deref(), Some("self"));
                                self_type.clone().expect("receiver requires an impl")
                            })
                            .replace(
                                "ApiType::Named(\"Self\", &[])",
                                self_type
                                    .as_deref()
                                    .unwrap_or("ApiType::Named(\"Self\", &[])")
                            )
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let output = function
                .return_type()
                .map(ty)
                .unwrap_or("ApiType::Tuple(&[])".into())
                .replace(
                    "ApiType::Named(\"Self\", &[])",
                    self_type
                        .as_deref()
                        .unwrap_or("ApiType::Named(\"Self\", &[])"),
                );
            let signature = function.syntax().text().to_string();
            let signature = &signature[signature.find("pub fn").unwrap()..];
            let doc = function.documentation(&text);
            assert!(!doc.is_empty(), "undocumented {name}");
            let range = api::name_range(&function.name().unwrap());
            let (start, end) = range;
            let generics = format!("&{:?}", generics);
            let constraints = format!("&[{}]", constraints.join(","));
            let predicates = predicates.join(",");
            let qualified_name = format!("std::{}::{export}", module.to_lowercase());
            writeln!(functions,"StandardFunctionSpec{{module:StandardModule::{module},name:{export:?},intrinsic:StandardIntrinsic::{intrinsic},type_params:{generics},arity:{arity},constraints:{constraints},api:&ApiFunction{{bounds:&[{predicates}],qualified_name:{qualified_name:?},uri:{uri:?},start:{start},end:{end},documentation:{doc:?},signature:{signature:?},params:&[{parameter_types}],result:{output}}}}},").unwrap();
            if params
                .first()
                .is_some_and(|p| p.name_text().as_deref() == Some("self"))
            {
                let method = name;
                assert!(arity > 0, "method needs a receiver");
                let receiver = owner.as_deref().expect("method owner");
                let receiver = match receiver {
                    "ArrayList" => "Array",
                    "LinkedHashMap" => "Map",
                    "LinkedHashSet" => "Set",
                    other => other,
                };
                assert!(
                    method_bindings.insert((receiver.to_owned(), method.clone())),
                    "duplicate method binding"
                );
                let receiver = if attribute(&function, "numeric").is_some() {
                    let builtin = match receiver {
                        "isize" => "ISize".to_owned(),
                        "usize" => "USize".to_owned(),
                        _ => {
                            let mut chars = receiver.chars();
                            chars.next().unwrap().to_uppercase().to_string() + chars.as_str()
                        }
                    };
                    format!("Builtin(BuiltinType::{builtin})")
                } else {
                    receiver.to_owned()
                };
                writeln!(methods,"StandardMethodSpec{{receiver:StandardMethodReceiver::{receiver},name:{method:?},intrinsic:StandardIntrinsic::{intrinsic},type_params:{generics},arity:{},constraints:{constraints}}},",arity-1).unwrap();
            }
        }
    }
    let out = format!(
        "pub const STANDARD_ITEMS:&[ApiItem]=&[{items}];\npub const STANDARD_TRAITS:&[ApiTrait]=&[{traits}];\nconst STANDARD_ENUMS:&[StandardEnumSpec]=&[{enums}];\nconst STANDARD_TYPE_CONSTRUCTORS:&[StandardTypeConstructorSpec]=&[{constructors}];\nconst STANDARD_FUNCTIONS:&[StandardFunctionSpec]=&[{functions}];\nconst STANDARD_METHODS:&[StandardMethodSpec]=&[{methods}];\npub const STANDARD_SOURCES:&[(&str,&str)]=&[{sources}];"
    );
    let out = format!(
        "{out}\npub const STANDARD_IMPLEMENTATIONS:&[super::declarations::ApiImplementation]=&[{implementations}];"
    );
    std::fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("standard_api.rs"),
        out,
    )
    .unwrap();
}

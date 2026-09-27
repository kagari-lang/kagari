//! Source-owned native collection implementations with sealed execution bindings.
use super::*;

pub fn declaration(
    def: &ast::ImplBlock,
    module: &str,
    uri: &str,
    text: &str,
    items: &mut String,
    output: &mut String,
) {
    let interface = def.trait_ref().unwrap();
    let protocol = interface.path_text().unwrap();
    let target = def.target_type().unwrap();
    let owner = target.name_text().unwrap();
    let parameters = def
        .generic_params()
        .into_iter()
        .flat_map(|p| p.params().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let names = parameters
        .iter()
        .map(|p| p.name_text().unwrap())
        .collect::<Vec<_>>();
    assert!(def.where_clause().is_none() && def.associated_consts().next().is_none());
    assert!(matches!(protocol.as_str(), "Iterable" | "FromIterator"));
    let expected_module = match owner.as_str() {
        "Array" | "MutableArray" => "array",
        "Map" | "MutableMap" => "map",
        "Set" | "MutableSet" => "set",
        "String" if protocol == "Iterable" => "string",
        "Result" if protocol == "FromIterator" => "result",
        "Option" if protocol == "FromIterator" => "option",
        _ => panic!("unsupported native implementation {protocol} for {owner}"),
    };
    assert_eq!(module, expected_module);
    let named = |name: &str| format!("ApiType::Named({name:?}, &[])");
    let (arguments, element): (Vec<String>, String) = match owner.as_str() {
        "Array" | "MutableArray" | "Set" | "MutableSet" => {
            assert_eq!(names.len(), 1);
            (vec![named(&names[0])], named(&names[0]))
        }
        "Map" | "MutableMap" => {
            assert_eq!(names.len(), 2);
            let args = names.iter().map(|n| named(n)).collect::<Vec<_>>();
            (
                args.clone(),
                format!("ApiType::Tuple(&[{}])", args.join(",")),
            )
        }
        "String" => {
            assert!(names.is_empty());
            (vec![], named("String"))
        }
        "Result" => {
            assert_eq!(names.len(), 3);
            (
                vec![named(&names[2]), named(&names[1])],
                format!(
                    "ApiType::Named(\"Result\", &[{},{}])",
                    named(&names[0]),
                    named(&names[1])
                ),
            )
        }
        "Option" => {
            assert_eq!(names.len(), 2);
            (
                vec![named(&names[1])],
                format!("ApiType::Named(\"Option\", &[{}])", named(&names[0])),
            )
        }
        _ => unreachable!(),
    };
    let target = ty(target);
    assert_eq!(
        target,
        format!("ApiType::Named({owner:?}, &[{}])", arguments.join(","))
    );
    let mut constraints = Vec::new();
    for (index, parameter) in parameters.iter().enumerate() {
        let bounds = api::bounds(parameter.bounds());
        let expected = if matches!(owner.as_str(), "Map" | "MutableMap" | "Set" | "MutableSet")
            && index == 0
        {
            "ApiBound{name:\"Eq\",args:&[],bindings:&[]},ApiBound{name:\"Hash\",args:&[],bindings:&[]}".into()
        } else if matches!(owner.as_str(), "Result" | "Option") && index == parameters.len() - 1 {
            format!(
                "ApiBound{{name:\"FromIterator\",args:&[{}],bindings:&[]}}",
                named(&names[0])
            )
        } else {
            String::new()
        };
        assert_eq!(bounds, expected, "native implementation bounds");
        if !bounds.is_empty() {
            constraints.push(format!("({},&[{bounds}])", named(&names[index])));
        }
    }
    let trait_arguments = interface
        .generic_args()
        .into_iter()
        .flat_map(|a| a.args().collect::<Vec<_>>())
        .map(ty)
        .collect::<Vec<_>>();
    assert_eq!(
        trait_arguments,
        if protocol == "Iterable" {
            vec![]
        } else {
            vec![element.clone()]
        }
    );
    assert!(
        interface
            .generic_args()
            .is_none_or(|a| a.bindings().next().is_none())
    );
    let path_owner = format!("{protocol} for {owner}");
    let mut associated = Vec::new();
    let members = def.associated_types().collect::<Vec<_>>();
    assert_eq!(members.len(), if protocol == "Iterable" { 2 } else { 0 });
    for (member, expected_name, expected_type) in
        members.iter().zip(["Item", "Iter"]).map(|(m, n)| {
            (
                m,
                n,
                if n == "Item" {
                    element.clone()
                } else {
                    format!("ApiType::Named(\"Iter\", &[{element}])")
                },
            )
        })
    {
        assert_eq!(member.name_text().as_deref(), Some(expected_name));
        assert!(
            member.bounds().is_none()
                && member.generic_params().is_none()
                && member.where_clause().is_none()
        );
        assert_eq!(ty(member.ty().unwrap()), expected_type);
        let item = api::item(
            member,
            member.name().unwrap(),
            module,
            uri,
            text,
            &[
                ("Impl", path_owner.clone()),
                ("AssociatedType", expected_name.into()),
            ],
        );
        writeln!(items, "{item},").unwrap();
        associated.push(format!("({item},{expected_type})"));
    }
    let methods = def.methods().collect::<Vec<_>>();
    assert_eq!(methods.len(), 1);
    let method = &methods[0];
    let name = if protocol == "Iterable" {
        "iter"
    } else {
        "from_iter"
    };
    let binding = match protocol.as_str() {
        "Iterable" => "CollectionIter",
        _ if owner == "Result" => "ResultFromIterator",
        _ if owner == "Option" => "OptionFromIterator",
        _ => "CollectionFromIterator",
    };
    assert_eq!(method.name_text().as_deref(), Some(name));
    assert_eq!(attribute(method, "intrinsic").as_deref(), Some(binding));
    assert_eq!(method.visibility(), ast::Visibility::Private);
    assert!(method.body().is_none() && method.where_clause().is_none());
    let method_generics = method
        .generic_params()
        .into_iter()
        .flat_map(|p| p.params().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let params = method.param_list().unwrap().params().collect::<Vec<_>>();
    assert_eq!(params.len(), 1);
    let (generics, param_type, result) = if protocol == "Iterable" {
        assert!(method_generics.is_empty());
        assert_eq!(params[0].name_text().as_deref(), Some("self"));
        assert!(params[0].ty().is_none());
        (
            String::new(),
            target.clone(),
            format!("ApiType::Named(\"Iter\", &[{element}])"),
        )
    } else {
        assert_eq!(method_generics.len(), 1);
        let generic = &method_generics[0];
        let name = generic.name_text().unwrap();
        let bounds = api::bounds(generic.bounds());
        assert_eq!(
            bounds,
            format!("ApiBound{{name:\"Iterable\",args:&[],bindings:&[(\"Item\",{element})]}}")
        );
        assert_eq!(ty(params[0].ty().unwrap()), named(&name));
        let key = format!("@bound:{name}");
        (
            format!(
                "super::declarations::ApiGeneric{{name:{name:?},bounds:&[{bounds}],projection_key:{key:?}}}"
            ),
            named(&name),
            named("Self"),
        )
    };
    assert_eq!(ty(method.return_type().unwrap()), result);
    let item = api::item(
        method,
        method.name().unwrap(),
        module,
        uri,
        text,
        &[("Impl", path_owner), ("Method", name.into())],
    );
    writeln!(items, "{item},").unwrap();
    let param_name = params[0].name_text().unwrap();
    let trait_arguments = trait_arguments.join(",");
    let constraints = constraints.join(",");
    let associated = associated.join(",");
    writeln!(output, "super::declarations::ApiImplementation{{interface:{protocol:?},trait_arguments:&[{trait_arguments}],bounds:&[{constraints}],generics:&{names:?},target:{target},associated_types:&[{associated}],methods:&[ApiMethod{{item:{item},iterator:None,generics:&[{generics}],bounds:&[],params:&[ApiParameter{{name:{param_name:?},ty:{param_type}}}],result:{result}}}]}},").unwrap();
}

use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, associated_type_id,
};
use kagari_types::{
    scalar::BuiltinType,
    ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty},
};
use std::collections::BTreeMap;
fn owner(name: &str) -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity::single_file("substitution.kgr"),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

fn parameter(owner: &DefinitionPath, position: usize) -> Ty {
    Ty::Parameter {
        owner: owner.clone(),
        position,
    }
}

#[test]
fn impl_instantiation_preserves_method_generics_and_substitutes_their_bounds() {
    use kagari_contract::types::InterfaceTable;
    use kagari_types::{
        callable::CallableImplementation,
        declaration::{FnDecl, Param},
    };
    let implementation = owner("Impl");
    let method = associated_type_id(&implementation, "construct");
    let interface = owner("Iterable");
    let item = associated_type_id(&interface, "Item");
    let outer = GenericParam {
        owner: implementation.clone(),
        position: 0,
    };
    let local = GenericParam {
        owner: method.clone(),
        position: 0,
    };
    let source = parameter(&method, 0);
    let input = parameter(&implementation, 0);
    let table = InterfaceTable {
        associated_type_families: vec![],
        associated_consts: vec![],
        host_bridge: false,
        declaration: implementation.clone(),
        name: "Impl".into(),
        generic_params: vec![outer.clone()],
        bounds: vec![],
        trait_type: Ty::Trait(NominalTy {
            declaration: interface.clone(),
            arguments: vec![],
            associated_types: BTreeMap::new(),
        }),
        for_type: Ty::Tuple(vec![input.clone()]),
        methods: vec![FnDecl {
            method_policy: Default::default(),
            name: "construct".into(),
            implementation: CallableImplementation::Script,
            generic_params: vec![local.clone()],
            bounds: vec![GenericBound {
                ty: source.clone(),
                constraints: vec![Constraint::Trait(NominalTy {
                    declaration: interface,
                    arguments: vec![],
                    associated_types: BTreeMap::from([(item.clone(), input.clone())]),
                })],
            }],
            params: vec![Param {
                name: "source".into(),
                mutable: false,
                ty: source.clone(),
            }],
            return_type: Ty::Tuple(vec![input]),
        }],
    };
    let number = Ty::Builtin(BuiltinType::I32);
    let applied = table.instantiate(std::slice::from_ref(&number)).unwrap();
    assert_eq!(applied.for_type, Ty::Tuple(vec![number.clone()]));
    assert!(applied.generic_params.is_empty());
    let method = &applied.methods[0];
    assert_eq!(method.generic_params, [local]);
    assert_eq!(method.params[0].ty, source);
    assert_eq!(method.return_type, Ty::Tuple(vec![number.clone()]));
    assert_eq!(method.bounds[0].ty, source);
    let Constraint::Trait(bound) = &method.bounds[0].constraints[0] else {
        panic!("Iterable bound")
    };
    assert_eq!(bound.associated_types[&item], number);
}

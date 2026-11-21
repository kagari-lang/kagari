use super::impl_headers_overlap;
use crate::{
    range::RangeKind,
    scalar::BuiltinType,
    ty::{
        NominalTy, Ty,
        substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES},
    },
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId,
};
use std::slice::from_ref;

const INT: Ty = Ty::Builtin(BuiltinType::I32);
const BOOL: Ty = Ty::Builtin(BuiltinType::Bool);

fn definitions() -> (DefinitionId, DefinitionId) {
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("overlap.kgr"))
        .unwrap();
    let interface = context
        .intern_child(root, DefinitionKind::Trait, "Traits", 0)
        .unwrap();
    let item = context
        .intern_child(interface, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    (interface, item)
}

fn parameter(owner: DefinitionId, position: usize) -> Ty {
    Ty::Parameter { owner, position }
}

fn check(left: (&Ty, &[Ty]), right: (&Ty, &[Ty]), expected: bool) {
    for ((a, args), (b, other)) in [(left, right), (right, left)] {
        assert_eq!(impl_headers_overlap(a, args, b, other), expected);
    }
}

#[test]
fn impl_headers_overlap_scopes_and_shares_nested_bindings() {
    let (owner, other) = definitions();
    let t = parameter(owner, 0);
    // The same ID on opposite sides can denote different concrete types.
    check((&t, &[BOOL]), (&INT, from_ref(&t)), true);
    check((&t, from_ref(&t)), (&INT, &[BOOL]), false);
    check(
        (&t, &[parameter(owner, 1), parameter(other, 0)]),
        (&INT, &[BOOL, BOOL]),
        true,
    );

    // Left T = Array<right T>; both later arguments constrain the element.
    let array = Ty::Array(Box::new(t.clone()));
    let ints = Ty::Array(Box::new(INT));
    let repeated = [t.clone(), INT];
    check((&t, &repeated), (&array, &[ints.clone(), t.clone()]), true);
    check(
        (&t, &[t.clone(), BOOL]),
        (&array, &[ints, t.clone()]),
        false,
    );
}

#[test]
fn impl_headers_overlap_distinguishes_rigid_structure() {
    let (declaration, item) = definitions();
    let t = parameter(declaration, 0);
    let nominal = NominalTy {
        declaration,
        arguments: vec![t.clone()],
        associated_types: Default::default(),
    };
    check((&INT, &[]), (&INT, from_ref(&t)), false);
    check((&Ty::Host(item), &[]), (&Ty::SelfType(item), &[]), false);
    check(
        (&Ty::Struct(nominal.clone()), &[]),
        (&Ty::NativeObject(nominal.clone()), &[]),
        false,
    );
    check(
        (&Ty::Enum(nominal.clone()), &[]),
        (
            &Ty::Enum(NominalTy {
                declaration: item,
                ..nominal.clone()
            }),
            &[],
        ),
        false,
    );
    let trait_ty = |output| {
        Ty::Trait(NominalTy {
            associated_types: [(item, output)].into(),
            ..nominal.clone()
        })
    };
    check(
        (&trait_ty(t.clone()), &[BOOL]),
        (&trait_ty(INT), from_ref(&t)),
        false,
    );
    check(
        (&trait_ty(INT), &[]),
        (&Ty::Trait(nominal.clone()), &[]),
        false,
    );
    let function = |param, result| Ty::Function {
        params: vec![param],
        result: Box::new(result),
    };
    check(
        (&function(t.clone(), t.clone()), &[]),
        (&function(INT, BOOL), &[]),
        false,
    );
    check(
        (&Ty::Range(Box::new(t.clone()), RangeKind::From), &[]),
        (&Ty::Range(Box::new(INT), RangeKind::To), &[]),
        false,
    );
    check((&Ty::Tuple(vec![t]), &[]), (&Ty::Tuple(vec![]), &[]), false);
}

#[test]
fn impl_headers_overlap_keeps_projection_uncertainty_local() {
    let (declaration, member) = definitions();
    let t = parameter(declaration, 0);
    let projection = Ty::Projection {
        arguments: vec![],
        receiver: Box::new(t.clone()),
        interface: Box::new(NominalTy {
            declaration,
            arguments: vec![],
            associated_types: Default::default(),
        }),
        member,
    };
    check((&projection, &[]), (&INT, &[]), true);
    check(
        (&t, &[t.clone(), t.clone()]),
        (&projection, &[INT, BOOL]),
        false,
    );
}

#[test]
fn impl_headers_overlap_bounds_recursive_equations_and_input_size() {
    let (owner, _) = definitions();
    let t = parameter(owner, 0);
    let array = Ty::Array(Box::new(t.clone()));
    // X = Array<Y>, Y = Array<X>. Rechecking X must terminate conservatively.
    check(
        (&t, &[array.clone(), t.clone()]),
        (&array, &[t.clone(), array.clone()]),
        true,
    );
    let deep = (0..MAX_TYPE_DEPTH).fold(INT, |item, _| Ty::Array(Box::new(item)));
    check((&deep, &[]), (&BOOL, &[]), true);
    let many = vec![INT; MAX_TYPE_NODES];
    check((&INT, &many), (&BOOL, &many), true);
}

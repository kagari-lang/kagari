use crate::{
    declaration::verify::{types_in_scope, types_in_scope_in},
    host_interface::host_type_identity,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, associated_type_id,
    metadata::scope_record, table::DefinitionTableBuilder,
};

fn interface() -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity::single_file("scope"),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Output".into(),
            occurrence: 0,
        }],
    }
}

#[test]
fn scoped_validation_preserves_kind_member_and_host_identity_rules() {
    let declaration = interface();
    let member = associated_type_id(&declaration, "Item");
    let mut nominal = NominalTy {
        declaration,
        arguments: vec![],
        associated_types: [(member, Ty::Builtin(BuiltinType::I32))].into(),
    };
    let mut cases = vec![(Ty::Trait(nominal.clone()), true)];
    let (mut member, value) = nominal.associated_types.pop_first().unwrap();
    member.path.last_mut().unwrap().occurrence = 1;
    nominal.associated_types.insert(member, value);
    cases.push((Ty::Trait(nominal), false));
    let host = host_type_identity("game.Entity");
    cases.push((Ty::Host(host.clone()), true));
    let mut wrong_host = host;
    wrong_host.path.last_mut().unwrap().kind = DefinitionKind::Function;
    cases.push((Ty::Host(wrong_host), false));
    let mut wrong_nominal = interface();
    wrong_nominal.path.last_mut().unwrap().kind = DefinitionKind::Struct;
    cases.push((
        Ty::Trait(NominalTy {
            declaration: wrong_nominal,
            arguments: vec![],
            associated_types: Default::default(),
        }),
        false,
    ));
    for (ty, accepted) in cases {
        let cancel = Default::default();
        assert_eq!(types_in_scope([&ty], &[], &cancel), accepted);
        let scoped = scope_record(&ty, &cancel).unwrap();
        assert_eq!(
            types_in_scope_in([scoped.records()], &[], &cancel, Some(scoped.definitions())),
            accepted
        );
        let foreign = DefinitionTableBuilder::new().unwrap().freeze();
        assert!(!types_in_scope_in(
            [scoped.records()],
            &[],
            &cancel,
            Some(&foreign)
        ));
        assert!(!types_in_scope_in([scoped.records()], &[], &cancel, None));
    }
}

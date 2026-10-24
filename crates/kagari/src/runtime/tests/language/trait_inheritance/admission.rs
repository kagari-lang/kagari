use super::compile;
use kagari_bytecode::program::verify_program;
use kagari_contract::types::PublicItem;
use kagari_types::ty::Ty;

#[test]
fn artifact_inheritance_cycles_and_missing_parent_implementations_are_rejected() {
    let artifact = compile("pub trait Parent {} pub trait Child: Parent {} struct S {} impl Parent for S {} impl Child for S {} fn main() {}").unwrap();
    for cycle in [true, false] {
        let mut program = artifact.records().program.clone();
        let module = &mut program.modules[program.root.index()];
        if cycle {
            let parent = module
                .public_items
                .iter()
                .find_map(|item| match item {
                    PublicItem::Trait(ty) if ty.name == "Child" => Some(ty.supertraits[0].clone()),
                    _ => None,
                })
                .unwrap();
            let PublicItem::Trait(ty) = module
                .public_items
                .iter_mut()
                .find(|item| matches!(item, PublicItem::Trait(ty) if ty.name == "Parent"))
                .unwrap()
            else {
                unreachable!()
            };
            ty.supertraits.push(parent);
        } else {
            module.public_items.retain(|item| !matches!(item, PublicItem::InterfaceTable(table) if matches!(&table.trait_type, Ty::Trait(ty) if artifact.definitions().resolve(ty.declaration).unwrap().last().unwrap().name == "Parent")));
        }
        assert!(verify_program(&program, artifact.definitions(), &Default::default()).is_err());
    }
}

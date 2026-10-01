use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::{NativeWitness, NativeWitnessImplementation},
    scalar::BuiltinType,
    standard::{bindings::NativeDefaultMethod, intrinsic, traits::StandardTrait},
    types::{AbiType, ConstraintAbi, GenericBoundAbi},
};
use kagari_bytecode::{artifact::KbcArtifact, program::verify_program};
use kagari_common::identity::associated_type_id;

#[test]
fn list_join_rejects_forged_storage_conversion_and_next_contracts() {
    let mut checked = 0;
    for source in [
        "val source=[\"é\",\"😀\"];val result=consume(source);",
        "val source=Sequence{items:[\"é\",\"😀\"]};val result=consume(source);",
        "val source:List<String> =[\"é\",\"😀\"];val result=source.join(\"/\");",
        "val source:List<String> =Sequence{items:[\"é\",\"😀\"]};val result=source.join(\"/\");",
        "val source:MutableList<String> =[\"é\",\"😀\"];val result=source.join(\"/\");",
    ] {
        let program = common::bytecode_ok(&format!(
            r#"
struct Sequence<T> {{val items:ArrayList<T>}}
impl<T> Iterable for Sequence<T> {{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{{self.items.iter()}}}}
impl<T> Index<usize> for Sequence<T> {{type Output=T;fn index(self,index:usize)->T{{self.items[index]}}}}
impl<T> List<T> for Sequence<T> {{fn len(self)->usize{{self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn get(self,index:usize)->Option<T>{{self.items.get(index)}}}}
fn consume<L:List<String>>(source:L)->String{{source.join("/")}}
fn main(){{{source}}}
"#
        ));
        let root = program.root.index();
        let index = program.modules[root]
            .native_imports
            .iter()
            .position(|import| {
                import.binding == EngineNativeBinding::TraitDefault(NativeDefaultMethod::ListJoin)
            })
            .unwrap();
        let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
        for mutation in 0..18 {
            let mut forged = artifact.clone();
            let import = &mut forged.program.modules[root].native_imports[index];
            let witness = |kind| {
                import
                    .witnesses
                    .iter()
                    .position(|w| StandardTrait::from_id(&w.interface.declaration) == Some(kind))
                    .unwrap()
            };
            let iterable = witness(StandardTrait::Iterable);
            let iterator = witness(StandardTrait::Iterator);
            let list = witness(StandardTrait::List);
            let boolean = AbiType::Builtin(BuiltinType::Bool);
            match mutation {
                0 => import.binding_version -= 1,
                1 => import.signature.params[0] = boolean,
                2 => import.signature.params[1] = boolean,
                3 => import.signature.result = boolean,
                4 => import.signature.result = AbiType::Builtin(BuiltinType::Never),
                5 => import.instance.arguments[0] = boolean,
                6 => import.requirements.clear(),
                7 => {
                    let selected = &mut import.witnesses[iterable];
                    selected.interface.associated_types.insert(
                        associated_type_id(&selected.interface.declaration, "Item"),
                        boolean,
                    );
                }
                8 => {
                    import.witnesses.remove(list);
                }
                9 => {
                    import.witnesses.remove(iterable);
                }
                10 => {
                    import.witnesses.remove(iterator);
                }
                11 => import.witnesses[iterator].receiver = AbiType::Iter(Box::new(boolean)),
                12 => {
                    let selected = &mut import.witnesses[iterator];
                    selected.interface.associated_types.insert(
                        associated_type_id(&selected.interface.declaration, "Item"),
                        boolean,
                    );
                }
                13 => import.witnesses[iterable].implementation = NativeWitnessImplementation::Host,
                14 => import.witnesses[iterator]
                    .methods
                    .push(import.instance.clone()),
                15 => import.witnesses.push(NativeWitness {
                    receiver: boolean,
                    interface: intrinsic::applied(StandardTrait::Eq, vec![]),
                    implementation: NativeWitnessImplementation::Primitive,
                    methods: vec![],
                }),
                16 => import.binding = EngineNativeBinding::TraitDefault(NativeDefaultMethod::Join),
                _ => import.requirements.push(GenericBoundAbi {
                    ty: import.signature.params[0].clone(),
                    constraints: vec![ConstraintAbi::Trait(intrinsic::applied(
                        StandardTrait::Eq,
                        vec![],
                    ))],
                }),
            }
            let label = format!("{source} {mutation}");
            assert!(verify_program(&forged.program).is_err(), "{label}");
            let bytes = DefaultOptions::new()
                .with_fixint_encoding()
                .with_little_endian()
                .serialize(&forged)
                .unwrap();
            assert!(
                !KbcArtifact::from_bytes(&bytes)
                    .is_ok_and(|decoded| decoded.validate_for_loader(&Default::default()).is_ok()),
                "encoded {label}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 90);
}

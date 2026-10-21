//! Shared generic construction has its own capability, including methodless traits.
use super::{editable_artifact, execute};
use crate::{BytecodeArtifact, test_support::execution};
use kagari_bytecode::instruction::{BytecodeInstruction, ConstructionTarget};
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn shared_bound_construction_preserves_empty_and_projected_capabilities() {
    execute(
        r#"
trait Read { fn read(self) -> i32; }
trait Child: Read {}
trait Marker {}
trait Source { type Item: Read; }
struct Number { val value: i32 }
impl Read for Number { fn read(self) -> i32 { self.value } }
impl Read for i32 { fn read(self) -> i32 { self } }
impl Child for Number {}
impl Marker for Number {}
struct Holder<T> { val value: T }
impl<T: Read> Read for Holder<T> { fn read(self) -> i32 { self.value.read() } }
struct Tag {}
impl Source for Tag { type Item = Number; }
fn erase<T: Read>(value: T) -> Read { value }
trait Erase {
    fn erase<T: Read>(self, value: T) -> Read { erase(value) }
    fn parent<T: Child>(self, value: T) -> Read { value }
    fn keep<T: Read>(self, value: T) -> fn() -> Read { || value }
    fn item<S: Source>(self, source: S, value: S::Item) -> Read { value }
    fn marker<T: Marker>(self, value: T) -> Marker { value }
}
struct DefaultErase {}
impl Erase for DefaultErase {}
struct OverrideErase {}
impl Erase for OverrideErase { fn erase<T: Read>(self, value: T) -> Read { value } }
fn main() -> i32 {
    val a: Erase = DefaultErase {};
    val b: Erase = OverrideErase {};
    val value = Number { value: 7 };
    val boxed = a.erase(value);
    val kept = a.keep(value);
    val marker = a.marker(value);
    boxed.read() + b.erase(Holder { value: value }).read() + a.parent(value).read()
        + kept().read() + a.item(Tag {}, value).read() + a.erase(7).read()
}

"#,
    );
}

#[test]
fn bound_construction_requires_the_declared_capability_and_actual_input_type() {
    let artifact = execution::engine(Default::default())
        .compile_to_artifact(
            SourceFile::new(
                "bound-construction.kgr",
                r#"
trait Read { fn read(self) -> i32; }
impl Read for i32 { fn read(self) -> i32 { self } }
trait Erase {
    fn erase<T: Read, U>(self, value: T, other: U) -> Read { value }
}
impl Erase for i32 {}
fn main() -> i32 { val eraser: Erase = 0; eraser.erase(42, true).read() }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let (context, artifact) = editable_artifact(artifact);
    for mutation in 0..5 {
        let mut program = artifact.program.clone();
        let function = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .find(|function| {
                function.instructions.iter().any(|instruction| {
                    matches!(
                        instruction,
                        BytecodeInstruction::MakeInterface {
                            target: ConstructionTarget::Bound(_),
                            ..
                        }
                    )
                })
            })
            .unwrap();
        if mutation == 0 {
            function
                .metadata
                .semantic
                .generic
                .as_mut()
                .unwrap()
                .bounds
                .clear();
        } else {
            let other = function.metadata.parameter_slots[2];
            let (input, required) = function
                .instructions
                .iter_mut()
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::MakeInterface {
                        value,
                        target: ConstructionTarget::Bound(required),
                        ..
                    } => Some((value, required)),
                    _ => None,
                })
                .unwrap();
            match mutation {
                1 => *input = other,
                2 => required.receiver = Ty::Builtin(BuiltinType::Bool),
                3 => {
                    let Ty::Parameter { position, .. } = &mut required.receiver else {
                        panic!("generic receiver")
                    };
                    *position += 20;
                }
                4 => required
                    .interface
                    .arguments
                    .push(Ty::Builtin(BuiltinType::Bool)),
                _ => unreachable!(),
            }
        }
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn shared_bound_construction_reuses_intrinsic_and_existing_interfaces() {
    execute(
        r#"
trait Erase {
    fn cursor<T: Iterator<Item=i32>>(self, value: T) -> Iterator<Item=i32> { value }
    fn callback<F: Fn() -> i32>(self, value: F) -> Fn<(), Output=i32> { value }
}
struct Eraser {}
impl Erase for Eraser {}
fn main() -> i32 {
    val eraser: Erase = Eraser {};
    val cursor: Iterator<Item=i32> = Vec::from([21]).iter();
    val copied = eraser.cursor(cursor);
    val callback = eraser.callback(|| 21);
    match copied.next() { Some(value) => value + callback.call(()), None => 0 }
}
"#,
    );
}

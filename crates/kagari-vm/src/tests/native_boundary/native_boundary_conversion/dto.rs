use super::source_free_vm;
use kagari_runtime::{
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        collections::vector::ScriptVec,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        declarations::FunctionDecl,
        schema::{TypeRegistration, ValueField},
        typed::NativeContext,
        types::Type,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;

struct Packet {
    hp: i32,
    shared: ScriptVec<i32>,
}

impl KagariType for Packet {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(cx: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        cx.structure::<Self>("Packet", |cx| {
            Ok(vec![
                ValueField::new("hp", cx.dependency::<i32>()?),
                ValueField::new("shared", cx.dependency::<ScriptVec<i32>>()?),
            ])
        })
    }
}

impl IntoKagari for Packet {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        let mut fields = cx.write_struct(expected)?;
        fields.field("hp", self.hp)?;
        fields.field("shared", self.shared)?;
        fields.finish()
    }
}

impl FromKagari for Packet {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        let mut fields = cx.read_struct(expected, value)?;
        Ok(Self {
            hp: fields.field("hp")?,
            shared: fields.field("shared")?,
        })
    }
}

#[test]
fn nominal_dtos_use_typed_callbacks_while_explicit_handles_keep_shared_identity() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut module = ModuleBuilder::new("example::dto", &catalog).unwrap();
    let packet = module.register_type::<Packet>().unwrap();
    let update = module
        .define_function(
            FunctionDecl::new("update")
                .parameter("packet", packet.clone())
                .returns(packet),
        )
        .unwrap();
    module
        .bind(
            update,
            |cx: &mut NativeContext<'_>, (mut packet,): (Packet,)| -> NativeResult<Packet> {
                packet.hp += 1;
                packet.shared.push(cx, 42)?;
                cx.collect_garbage()?;
                Ok(packet)
            },
        )
        .unwrap();
    let (vm, owner) = source_free_vm(
        r#"
        use example::dto::{Packet, update};
        pub fn main() -> (i32, i32, u64) {
            val input = Packet { hp: 7, shared: Vec::from([1]) };
            val output = update(input);
            (input.hp, output.hp, input.shared.len())
        }
        "#,
        &module.finish().unwrap(),
    );
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    assert_eq!(
        vm.execute_typed::<_, (i32, i32, usize)>(&owner, "main", ())
            .unwrap(),
        (7, 8, 2)
    );
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

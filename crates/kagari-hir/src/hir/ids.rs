macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(u32);

        impl $name {
            pub(crate) fn new(index: usize) -> Self {
                Self(index as u32)
            }

            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id_newtype!(FunctionId);
id_newtype!(ParamId);
id_newtype!(LocalId);
id_newtype!(StructId);
id_newtype!(EnumId);
id_newtype!(BlockId);
id_newtype!(ExprId);
id_newtype!(PlaceId);
id_newtype!(StmtId);
id_newtype!(PatternId);
id_newtype!(TypeRefId);

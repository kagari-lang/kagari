use serde::{Deserialize, Serialize};

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(u32);

        impl $name {
            pub fn new(index: usize) -> Self {
                Self(index as u32)
            }

            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id_newtype!(BlockId);
id_newtype!(InstanceId);
id_newtype!(LocalId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleSlotId(u32);

impl ModuleSlotId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

id_newtype!(TempId);

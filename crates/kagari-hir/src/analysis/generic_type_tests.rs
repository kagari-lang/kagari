use super::*;

use crate::types::TypeId;
use kagari_abi::scalar::BuiltinType;
use kagari_common::{
    collection::CollectionAccess,
    source_database::{SourceDatabase, SourceLayer},
};

mod completion;
mod context;
mod contracts;
mod operators;
mod recovery;

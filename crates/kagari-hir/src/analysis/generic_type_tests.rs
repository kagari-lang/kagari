use super::*;

use crate::types::TypeId;
use kagari_contract::scalar::BuiltinType;
use {
    kagari_common::collection::CollectionAccess,
    kagari_source::source_database::{SourceDatabase, SourceLayer},
};

mod completion;
mod context;
mod contracts;
mod operators;
mod recovery;

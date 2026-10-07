//! Source-oriented nodes stored by one lowering, before semantic resolution.
//!
//! Start at [`item::Module`] for declaration collections, [`body::Body`] for node
//! storage and [`ids`] for lookup rules. These are owned Rust records, unlike the
//! typed CST views in `kagari-syntax`. Child IDs address other records; they do not
//! own subtrees. Name/type/call results live separately in [`crate::resolver::resolved::ResolvedNames`]
//! and [`crate::typeck::table::TypeTable`].

pub mod body;
pub mod expr;
pub mod ids;
pub mod item;
pub mod pattern;
pub mod place;
pub mod stmt;
pub mod ty;
pub mod writeability;

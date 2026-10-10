//! Source-oriented nodes stored by one lowering, before semantic resolution.
//!
//! Start at [`item::Module`] for declaration collections, [`body::Body`] for node
//! storage and [`ids`] for lookup rules. These are owned Rust records, unlike the
//! typed CST views in `kagari-syntax`. Child IDs address other records; they do not
//! own subtrees. Name/type/call results live separately in [`crate::resolver::resolved::ResolvedNames`]
//! and [`crate::typeck::table::TypeTable`].

//! # Source-to-field reading order
//!
//! For `fn add(x: i32) -> i32 { val y = x + 1; y }`, start with
//! item::function::Function: its example maps each signature/body field to source.
//! Follow its body link to stmt::BlockData, its statement to stmt::StmtKind, and
//! its initializer to expr::ExprKind. TypeRefId links enter ty::TypeKind rather
//! than checked types. Assignment destinations use place::PlaceKind; match/for/
//! binding-condition inputs use pattern::PatternKind.
//!
//! Each model documents source fragments and symbolic payloads, including empty
//! buffers, omitted syntax, allocated identities and recovery. Symbolic IDs are
//! illustrative, not promised numerical allocation order. Structs/enums/fields
//! and variants are declaration models in item::adt; traits/impls, associated
//! members and bounds are in item::behavior. Some retained fields/models are
//! currently unpopulated, and lowering can preserve syntax rejected by checking.
//! Examples distinguish those cases from implemented language behavior.

pub mod body;
pub mod expr;
pub mod ids;
pub mod item;
pub mod pattern;
pub mod place;
pub mod stmt;
pub mod ty;
pub mod writeability;

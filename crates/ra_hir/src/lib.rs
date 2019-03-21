//! HIR (previously known as descriptors) provides a high-level object oriented
//! access to Rust code.
//!
//! The principal difference between HIR and syntax trees is that HIR is bound
//! to a particular crate instance. That is, it has cfg flags and features
//! applied. So, the relation between syntax and HIR is many-to-one.

macro_rules! impl_froms {
    ($e:ident: $($v:ident), *) => {
        $(
            impl From<$v> for $e {
                fn from(it: $v) -> $e {
                    $e::$v(it)
                }
            }
        )*
    }
}

pub mod db;
#[macro_use]
pub mod mock;
mod path;
pub mod source_binder;

mod ids;
mod name;
mod nameres;
mod adt;
mod type_alias;
mod type_ref;
mod ty;
mod impl_block;
mod expr;
mod generics;
mod docs;
mod resolve;
pub mod diagnostics;

mod code_model_api;
mod code_model_impl;

#[cfg(test)]
mod marks;

use crate::{
    db::{HirDatabase, PersistentHirDatabase},
    name::{AsName, KnownName},
    ids::{SourceItemId, SourceFileItems},
};

pub use self::{
    path::{Path, PathKind},
    name::Name,
    ids::{HirFileId, MacroCallId, MacroCallLoc, HirInterner},
    nameres::{PerNs, Namespace},
    ty::{Ty, Substs, display::HirDisplay},
    impl_block::{ImplBlock, ImplItem},
    docs::{Docs, Documentation},
    adt::AdtDef,
    expr::{ExprScopes, ScopesWithSourceMap, ScopeEntryWithSyntax},
    resolve::{Resolver, Resolution},
};

pub use self::code_model_api::{
    Crate, CrateDependency,
    Module, ModuleDef, ModuleSource, Problem,
    Struct, Enum, EnumVariant,
    Function, FnSignature,
    StructField, FieldSource,
    Static, Const, ConstSignature,
    Trait, TypeAlias,
};

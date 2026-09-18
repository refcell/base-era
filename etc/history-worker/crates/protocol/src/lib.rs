#![doc = include_str!("../../../README.md")]

/// Wire protocol version.
pub const VERSION: u16 = 2;

mod types;
pub use types::*;

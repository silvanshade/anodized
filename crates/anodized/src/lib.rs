#![doc = include_str!("../README.md")]
#![no_std]

#[cfg(feature = "logic")]
extern crate alloc;

#[cfg(feature = "logic")]
pub use anodized_logic as logic;
#[cfg(feature = "logic")]
pub use anodized_logic::arithmetic;
pub use anodized_macros::spec;

pub mod __;
pub mod result;
pub mod types;

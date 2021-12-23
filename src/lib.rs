//! The code providing timelock primitives
//! used by [streamflow.finance](https://streamflow.finance).

/// Entrypoint
#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;
/// Errors
pub(crate) mod error;
/// Structs and data
pub mod state;
/// Utility functions
pub(crate) mod utils;

pub mod cancel;
pub mod create;
pub mod topup;
pub mod transfer;
pub mod withdraw;

pub(crate) const MAX_STRING_SIZE: usize = 200;

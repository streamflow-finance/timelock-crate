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

pub mod cancel_stream;
pub mod create_stream;
pub mod topup_stream;
pub mod transfer_recipient;
pub mod withdraw_stream;

pub(crate) const MAX_STRING_SIZE: usize = 200;

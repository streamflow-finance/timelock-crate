use solana_program::{msg, program_error::ProgramError};
use thiserror::Error;

#[derive(Error, Debug, Copy, Clone)]
pub(crate) enum SfError {
    #[error("Accounts not writable!")]
    AccountsNotWritable,

    #[error("Invalid Metadata!")]
    InvalidMetadata,

    #[error("Invalid metadata account")]
    InvalidMetadataAccount,

    #[error("Metadata mismatched with given accounts")]
    MetadataAccountMismatch,

    #[error("Invalid escrow account")]
    InvalidEscrowAccount,

    #[error("Sender mint does not match accounts mint!")]
    MintMismatch,

    #[error("Recipient not transferable for account")]
    TransferNotAllowed,

    #[error("Stream closed")]
    StreamClosed,

    #[error("Invalid Streamflow Treasury accounts")]
    InvalidTreasury,

    #[error("Stream name too long")]
    StreamNameTooLong,

    #[error("Given timestamps are invalid")]
    InvalidTimestamps,

    #[error("Deposited amount must be <= Total amount")]
    InvalidDeposit,

    #[error("Amount cannot be zero")]
    AmountIsZero,

    #[error("Amount requested is larger than available")]
    AmountMoreThanAvailable,
}

impl From<SfError> for ProgramError {
    fn from(e: SfError) -> Self {
        msg!(&e.to_string());
        ProgramError::Custom(e as u32)
    }
}

use solana_program::{msg, program_error::ProgramError};
use thiserror::Error;

#[derive(Error, Debug, Copy, Clone)]
pub enum SfError {
    #[error("Accounts not writable!")]
    AccountsNotWritable = 0x60,

    #[error("Invalid Metadata!")]
    InvalidMetadata = 0x61,

    #[error("Invalid metadata account")]
    InvalidMetadataAccount = 0x62,

    #[error("Provided accounts don't match the ones in contract.")]
    MetadataAccountMismatch = 0x63,

    #[error("Invalid escrow account")]
    InvalidEscrowAccount = 0x64,

    #[error("Provided account(s) is/are not valid associated token accounts.")]
    NotAssociated = 0x65,

    #[error("Sender mint does not match accounts mint!")]
    MintMismatch = 0x66,

    #[error("Recipient not transferable for account")]
    TransferNotAllowed = 0x67,

    #[error("Stream closed")]
    StreamClosed = 0x68,

    #[error("Invalid Streamflow Treasury accounts supplied")]
    InvalidTreasury = 0x69,

    #[error("Given timestamps are invalid")]
    InvalidTimestamps = 0x70,

    #[error("Deposited amount must be <= Total amount")]
    InvalidDeposit = 0x71,

    #[error("Amount cannot be zero")]
    AmountIsZero = 0x72,

    #[error("Amount requested is larger than available")]
    AmountMoreThanAvailable = 0x73,
}

impl From<SfError> for ProgramError {
    fn from(e: SfError) -> Self {
        msg!(&e.to_string());
        ProgramError::Custom(e as u32)
    }
}

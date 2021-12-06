use solana_program::msg;
use solana_program::program_error::ProgramError;
use thiserror::Error;

#[derive(Error, Debug, Copy, Clone)]
pub enum StreamFlowError {
    #[error("Accounts not writable!")]
    AccountsNotWritable,

    #[error("Invalid Metadata!")]
    InvalidMetadata,

    #[error("Sender mint does not match accounts mint!")]
    MintMismatch,

    #[error("Recipient not transferable for account")]
    TransferNotAllowed,

    #[error("Stream closed")]
    StreamClosed,
}

impl From<StreamFlowError> for ProgramError {
    fn from(e: StreamFlowError) -> Self {
        msg!(&e.to_string());
        ProgramError::Custom(e as u32)
    }
}

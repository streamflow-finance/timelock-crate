use solana_program::program_error::ProgramError;
use thiserror::Error;

#[derive(Error, Debug, Copy, Clone)]
pub enum StreamFlowError {
    #[error("Accounts not writable!")]
    AccountsNotWritable = 1,

    #[error("Invalid Metadata!")]
    InvalidMetaData = 2,

    #[error("Sender mint does not match accounts mint!")]
    MintMismatch = 3,
}

impl From<StreamFlowError> for ProgramError {
    fn from(e: StreamFlowError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
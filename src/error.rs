use solana_program::program_error::ProgramError;
use thiserror::Error;
use  solana_program::msg;

#[derive(Error, Debug, Copy, Clone)]
pub enum StreamFlowError {
    #[error("Accounts not writable!")]
    AccountsNotWritable,

    #[error("Invalid Metadata!")]
    InvalidMetaData,

    #[error("Sender mint does not match accounts mint!")]
    MintMismatch,
}

impl From<StreamFlowError> for ProgramError {
    fn from(e: StreamFlowError) -> Self {
        msg!(&e.to_string());
        ProgramError::Custom(e as u32)
    }
}
use borsh::BorshDeserialize;
use solana_program::program_error::ProgramError;
use std::convert::TryInto;

use crate::state::CreateParams;

pub enum StreamInstruction {
    /// Create token stream with configured parameters. Tokens are transferred to program derived
    /// account from which they are unlocked for recipient to withdraw.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner/delegate
    ///   0. `[writable, signer]` The source account.
    ///   1. `[writable]` Source associated token account.
    ///   2. `[]` The destination account.
    ///   3. `[]` Destination associated token account.
    ///   4. `[writable, signer]` Account used to store stream metadata. Expected to be unitialized
    ///   5. `[writable]` Metadata associated token account. Expected to be unitialized
    ///   6. `[]` The Streamflow treasury account.
    ///   7. `[writable]` The Streamflow associated token account.
    ///   8. `[]` The Partner treasury account.
    ///   9. `[writable]` The Partner associated token account.
    ///   10. `[]` The token mint account
    ///   11. `[]` The streamflow internal account that handles fees for specified partners
    ///   12. `[]` The rent sysvar account
    ///   13. `[]` The token program (SPL) in case associated token account is created
    ///   14. `[]` The Associated token program in case associated token account is created
    ///   15. `[]` The solana system program needed for account creation
    Create { create_params: CreateParams },

    /// Withdraws from initialized stream, released specified amount of unlocked tokens from the
    /// escrow account.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner/delegate
    ///   0. `[writable, signer]` The account invoking instruction.
    ///   1. `[]` The destination account.
    ///   2. `[]` Destination associated token account.
    ///   3. `[writable]` Account used to store stream metadata.
    ///   4. `[writable]` Metadata associated token account.
    ///   5. `[writable]` The Streamflow treasury account.
    ///   6. `[writable]` The Streamflow associated token account.
    ///   7. `[]` The Partner treasury account.
    ///   8. `[writable]` The Partner associated token account.
    ///   9. `[]` The token mint account
    ///   1. `[]` The token program (SPL) in case associated token account is created
    Withdraw { amount: u64 },

    /// Cancels given stream, transferring all unlocked tokens and corresponding fees to
    /// recipient and fee treasuries. Remaining unlocked tokens are returned back to the
    /// source account.
    ///
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner/delegate
    ///   0. `[writable, signer]` The account invoking instruction.
    ///   1. `[]` The source account.
    ///   2. `[writable]` Source associated token account.
    ///   3. `[]` The destination account.
    ///   4. `[writable]` Destination associated token account.
    ///   5. `[writable]` Account used to store stream metadata.
    ///   6. `[writable]` Metadata associated token account.
    ///   7. `[writable]` The Streamflow treasury account.
    ///   8. `[writable]` The Streamflow associated token account.
    ///   9. `[]` The Partner treasury account.
    ///   10. `[writable]` The Partner associated token account.
    ///   11. `[]` The token mint account
    ///   12. `[]` The token program (SPL) in case associated token account is created
    Cancel,

    /// Transfers provided stream to a new recipient without invoking any unlocked token transfers
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner/delegate
    ///   0. `[writable, signer]` The account invoking instruction.
    ///   1. `[writable]` New destination account.
    ///   2. `[writable]` New destination token account.
    ///   3. `[writable]` Account used to store stream metadata.
    ///   4. `[]` The token mint account
    ///   5. `[]` The rent sysvar account
    ///   6. `[]` The token program (SPL) in case associated token account is created
    ///   7. `[]` The Associated token program in case associated token account is created
    ///   8. `[]` The solana system program needed for account creation
    Transfer,

    /// Adds more tokens to stream deposit if possible (set in create params). This increases
    /// the duration of the stream, making 'infinite' streams possible.  
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner/delegate
    ///   0. `[writable, signer]` The source account.
    ///   1. `[writable]` The source associated token account.
    ///   2. `[writable]` Account used to store stream metadata.
    ///   3. `[writable]` Metadata associated token account.
    ///   4. `[writable]` The Streamflow treasury account.
    ///   5. `[writable]` The Streamflow associated token account.
    ///   6. `[]` The Partner treasury account.
    ///   7. `[writable]` The Partner associated token account.
    ///   8. `[]` The token mint account
    ///   9. `[]` The token program (SPL) in case associated token account is created
    TopUp { amount: u64 },
}

impl StreamInstruction {
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        let (tag, rest) = input.split_first().ok_or(ProgramError::InvalidInstructionData)?;

        Ok(match tag {
            0 => Self::Create { create_params: CreateParams::try_from_slice(rest)? },
            1 => Self::Withdraw { amount: Self::unpack_amount(rest)? },
            2 => Self::Cancel,
            3 => Self::Transfer,
            4 => Self::TopUp { amount: Self::unpack_amount(rest)? },
            _ => return Err(ProgramError::InvalidInstructionData),
        })
    }

    pub fn unpack_amount(input: &[u8]) -> Result<u64, ProgramError> {
        let amount = input
            .get(..8)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(ProgramError::InvalidInstructionData)?;
        Ok(amount)
    }
}

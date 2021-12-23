use borsh::BorshSerialize;
use solana_program::{
    account_info::AccountInfo, borsh as solana_borsh, entrypoint::ProgramResult, msg,
    program::invoke, program_error::ProgramError, pubkey::Pubkey, system_program, sysvar,
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};

use crate::{error::SfError, state::TokenStreamData, utils::Invoker};

#[derive(Clone, Debug)]
pub struct TransferAccounts<'a> {
    /// Account invoking cancel.
    pub authority: AccountInfo<'a>,
    /// Wallet address of a new recipient
    pub recipient: AccountInfo<'a>,
    /// The associated token account address of a `new_recipient`
    pub recipient_tokens: AccountInfo<'a>,
    /// The account holding the stream parameters
    pub metadata: AccountInfo<'a>,
    /// The SPL token mint account
    pub mint: AccountInfo<'a>,
    /// The system Rent account
    pub rent: AccountInfo<'a>,
    /// The SPL token program
    pub token_program: AccountInfo<'a>,
    /// The Associated Token program needed in case associated
    /// account for the new recipient is being created.
    pub associated_token_program: AccountInfo<'a>,
    /// The Solana system program needed for account creation
    pub system_program: AccountInfo<'a>,
}

fn account_sanity_check(pid: &Pubkey, a: TransferAccounts) -> ProgramResult {
    msg!("Checking if all given accounts are correct");

    // These accounts must not be empty, and need to have correct ownership
    if a.metadata.data_is_empty() || a.metadata.owner != pid {
        return Err(SfError::InvalidMetadataAccount.into())
    }

    // We want these accounts to be writable
    if !a.authority.is_writable || !a.recipient_tokens.is_writable || !a.metadata.is_writable {
        return Err(SfError::AccountsNotWritable.into())
    }

    // Check if the associated token accounts are legit
    let recipient_tokens = get_associated_token_address(a.recipient.key, a.mint.key);

    if a.recipient_tokens.key != &recipient_tokens {
        return Err(SfError::MintMismatch.into())
    }

    // On-chain program ID checks
    if a.rent.key != &sysvar::rent::id() ||
        a.token_program.key != &spl_token::id() ||
        a.associated_token_program.key != &spl_associated_token_account::id() ||
        a.system_program.key != &system_program::id()
    {
        return Err(ProgramError::InvalidAccountData)
    }

    // Passed without touching the lasers
    Ok(())
}

fn metadata_sanity_check(acc: TransferAccounts, metadata: TokenStreamData) -> ProgramResult {
    msg!("Checking metadata for correctness");

    if acc.mint.key != &metadata.mint {
        return Err(SfError::MintMismatch.into())
    }

    // TODO: What else?

    // Passed without touching the lasers
    Ok(())
}

pub fn transfer_recipient(pid: &Pubkey, acc: TransferAccounts) -> ProgramResult {
    msg!("Transferring stream recipient");

    // Sanity checks
    account_sanity_check(pid, acc.clone())?;

    let mut data = acc.metadata.try_borrow_mut_data()?;
    let mut metadata: TokenStreamData = match solana_borsh::try_from_slice_unchecked(&data) {
        Ok(v) => v,
        Err(_) => return Err(SfError::InvalidMetadata.into()),
    };

    metadata_sanity_check(acc.clone(), metadata.clone())?;

    let transfer_authority = Invoker::new(
        acc.authority.key,
        &metadata.sender,
        &metadata.recipient,
        &metadata.streamflow_treasury,
        &metadata.partner,
    );
    if !transfer_authority.can_transfer(&metadata.ix) {
        return Err(SfError::TransferNotAllowed.into())
    }

    metadata.recipient = *acc.recipient.key;
    metadata.recipient_tokens = *acc.recipient_tokens.key;

    if acc.recipient_tokens.data_is_empty() {
        msg!("Initializing new recipient's associated token account");
        invoke(
            &create_associated_token_account(acc.authority.key, acc.recipient.key, acc.mint.key),
            &[
                acc.authority.clone(),
                acc.recipient_tokens.clone(),
                acc.recipient.clone(),
                acc.mint.clone(),
                acc.system_program.clone(),
                acc.token_program.clone(),
                acc.rent.clone(),
            ],
        )?;
    }

    let bytes = metadata.try_to_vec()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    msg!("Successfully transferred stream recipient");
    Ok(())
}

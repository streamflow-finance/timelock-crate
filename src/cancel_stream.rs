use std::str::FromStr;

use borsh::BorshSerialize;
use solana_program::{
    account_info::AccountInfo,
    borsh as solana_borsh,
    entrypoint::ProgramResult,
    msg,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar::{clock::Clock, Sysvar},
};
use spl_associated_token_account::get_associated_token_address;
use spl_token::amount_to_ui_amount;

use crate::{
    error::SfError,
    state::TokenStreamData,
    utils::{calculate_available, unpack_mint_account, Invoker},
    STRM_TREASURY,
};

#[derive(Clone, Debug)]
pub struct CancelAccounts<'a> {
    pub authority: AccountInfo<'a>,
    pub sender: AccountInfo<'a>,
    pub sender_tokens: AccountInfo<'a>,
    pub recipient: AccountInfo<'a>,
    pub recipient_tokens: AccountInfo<'a>,
    pub metadata: AccountInfo<'a>,
    pub escrow_tokens: AccountInfo<'a>,
    pub streamflow_treasury: AccountInfo<'a>,
    pub streamflow_treasury_tokens: AccountInfo<'a>,
    pub partner: AccountInfo<'a>,
    pub partner_tokens: AccountInfo<'a>,
    pub mint: AccountInfo<'a>,
    pub token_program: AccountInfo<'a>,
}

fn account_sanity_check(pid: &Pubkey, a: CancelAccounts) -> ProgramResult {
    msg!("Checking if all given accounts are correct");

    // These accounts must not be empty, and need to have correct ownership
    if a.escrow_tokens.data_is_empty() || a.escrow_tokens.owner != &spl_token::id() {
        return Err(SfError::InvalidEscrowAccount.into())
    }

    if a.metadata.data_is_empty() || a.metadata.owner != pid {
        return Err(SfError::InvalidMetadataAccount.into())
    }

    // We want these accounts to be writable
    if !a.authority.is_writable ||
        !a.recipient_tokens.is_writable ||
        !a.metadata.is_writable ||
        !a.escrow_tokens.is_writable ||
        !a.streamflow_treasury_tokens.is_writable ||
        !a.partner_tokens.is_writable
    {
        return Err(SfError::AccountsNotWritable.into())
    }

    // Check if the associated token accounts are legit
    let strm_treasury_pubkey = Pubkey::from_str(STRM_TREASURY).unwrap();
    let strm_treasury_tokens = get_associated_token_address(&strm_treasury_pubkey, a.mint.key);
    let sender_tokens = get_associated_token_address(a.sender.key, a.mint.key);
    let recipient_tokens = get_associated_token_address(a.recipient.key, a.mint.key);
    let partner_tokens = get_associated_token_address(a.partner.key, a.mint.key);

    if a.streamflow_treasury.key != &strm_treasury_pubkey ||
        a.streamflow_treasury_tokens.key != &strm_treasury_tokens
    {
        return Err(SfError::InvalidTreasury.into())
    }

    if a.sender_tokens.key != &sender_tokens ||
        a.recipient_tokens.key != &recipient_tokens ||
        a.partner_tokens.key != &partner_tokens
    {
        return Err(SfError::MintMismatch.into())
    }

    // Check escrow token account is legit
    // TODO: Needs a deterministic seed and metadata should become a PDA
    let escrow_tokens_pubkey = Pubkey::find_program_address(&[a.metadata.key.as_ref()], pid).0;
    if &escrow_tokens_pubkey != a.escrow_tokens.key {
        return Err(ProgramError::InvalidAccountData)
    }

    // On-chain program ID checks
    if a.token_program.key != &spl_token::id() {
        return Err(ProgramError::InvalidAccountData)
    }

    // Passed without touching the lasers
    Ok(())
}

fn metadata_sanity_check(acc: CancelAccounts, metadata: TokenStreamData) -> ProgramResult {
    // Compare that all the given accounts match the ones inside our metadata.
    if acc.recipient.key != &metadata.recipient ||
        acc.recipient_tokens.key != &metadata.recipient_tokens ||
        acc.mint.key != &metadata.mint ||
        acc.escrow_tokens.key != &metadata.escrow_tokens ||
        acc.streamflow_treasury.key != &metadata.streamflow_treasury ||
        acc.streamflow_treasury_tokens.key != &metadata.streamflow_treasury_tokens ||
        acc.partner.key != &metadata.partner ||
        acc.partner_tokens.key != &metadata.partner_tokens
    {
        return Err(SfError::MetadataAccountMismatch.into())
    }

    // TODO: What else?

    // Passed without touching the lasers
    Ok(())
}

/// Cancel an SPL Token stream
///
/// The function will read the instructions from the metadata account and see
/// if there are any unlocked funds. If so, they will be transferred to the
/// stream recipient.
pub fn cancel(pid: &Pubkey, acc: CancelAccounts) -> ProgramResult {
    msg!("Cancelling SPL token stream");

    let now = Clock::get()?.unix_timestamp as u64;
    let mint_info = unpack_mint_account(&acc.mint)?;

    // Sanity checks
    account_sanity_check(pid, acc.clone())?;

    let mut data = acc.metadata.try_borrow_mut_data()?;
    let mut metadata: TokenStreamData = match solana_borsh::try_from_slice_unchecked(&data) {
        Ok(v) => v,
        Err(_) => return Err(SfError::InvalidMetadata.into()),
    };

    metadata_sanity_check(acc.clone(), metadata.clone())?;

    // TODO: Check signer(s)

    // If stream is expired, anyone can close it
    if now < metadata.closable_at {
        msg!("Stream not yet expired, checking authorization");
        if !acc.authority.is_signer {
            return Err(ProgramError::MissingRequiredSignature)
        }
        let cancel_authority = Invoker::new(acc.authority.key, acc.sender.key, acc.recipient.key);
        if !cancel_authority.can_cancel(&metadata.ix) {
            return Err(ProgramError::InvalidAccountData)
        }
    }

    let recipient_available = calculate_available(
        now,
        metadata.ix.clone(),
        metadata.ix.deposited_amount,
        metadata.withdrawn_amount,
    );

    let streamflow_available = calculate_available(
        now,
        metadata.ix.clone(),
        metadata.streamflow_fee_total,
        metadata.streamflow_fee_withdrawn,
    );

    let partner_available = calculate_available(
        now,
        metadata.ix.clone(),
        metadata.partner_fee_total,
        metadata.partner_fee_withdrawn,
    );

    // TODO: Handle requested amounts.

    let recipient_remains = metadata.ix.deposited_amount - recipient_available;
    let streamflow_remains = metadata.streamflow_fee_total - streamflow_available;
    let partner_remains = metadata.partner_fee_total - partner_available;

    let escrow_tokens_bump = Pubkey::find_program_address(&[acc.metadata.key.as_ref()], pid).1;
    let seeds = [acc.metadata.key.as_ref(), &[escrow_tokens_bump]];

    if recipient_available > 0 {
        msg!("Transferring unlocked tokens to recipient");
        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.recipient_tokens.key,
                acc.escrow_tokens.key,
                &[],
                recipient_available,
            )?,
            &[
                acc.escrow_tokens.clone(),    // src
                acc.recipient_tokens.clone(), // dest
                acc.escrow_tokens.clone(),    // auth
                acc.token_program.clone(),    // program
            ],
            &[&seeds],
        )?;

        metadata.withdrawn_amount += recipient_available;
        metadata.last_withdrawn_at = now;
        msg!(
            "Withdrawn: {} {} tokens",
            amount_to_ui_amount(recipient_available, mint_info.decimals),
            metadata.mint
        );
        msg!(
            "Remaining: {} {} tokens",
            amount_to_ui_amount(
                metadata.ix.deposited_amount - metadata.withdrawn_amount,
                mint_info.decimals
            ),
            metadata.mint
        );
    }

    if streamflow_available > 0 {
        msg!("Transferring unlocked tokens to Streamflow treasury");
        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.streamflow_treasury_tokens.key,
                acc.escrow_tokens.key,
                &[],
                streamflow_available,
            )?,
            &[
                acc.escrow_tokens.clone(),              // src
                acc.streamflow_treasury_tokens.clone(), // dest
                acc.escrow_tokens.clone(),              // auth
                acc.token_program.clone(),              // program
            ],
            &[&seeds],
        )?;

        metadata.streamflow_fee_withdrawn += streamflow_available; // TODO: FIXME
        metadata.last_withdrawn_at = now;
        msg!(
            "Withdrawn: {} {} tokens",
            amount_to_ui_amount(streamflow_available, mint_info.decimals),
            metadata.mint
        );
        msg!(
            "Remaining: {} {} tokens",
            amount_to_ui_amount(
                metadata.streamflow_fee_total - metadata.streamflow_fee_withdrawn,
                mint_info.decimals
            ),
            metadata.mint
        );
    }

    if partner_available > 0 {
        msg!("Transferring unlocked tokens to partner");
        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.partner_tokens.key,
                acc.escrow_tokens.key,
                &[],
                partner_available,
            )?,
            &[
                acc.escrow_tokens.clone(),  // src
                acc.partner_tokens.clone(), // dest
                acc.escrow_tokens.clone(),  // auth
                acc.token_program.clone(),  // program
            ],
            &[&seeds],
        )?;

        metadata.partner_fee_withdrawn += partner_available;
        metadata.last_withdrawn_at = now;
        msg!(
            "Withdrawn: {} {} tokens",
            amount_to_ui_amount(partner_available, mint_info.decimals),
            metadata.mint
        );
        msg!(
            "Remaining: {} {} tokens",
            amount_to_ui_amount(
                metadata.partner_fee_total - metadata.partner_fee_withdrawn,
                mint_info.decimals
            ),
            metadata.mint
        );
    }

    if recipient_remains > 0 || streamflow_remains > 0 || partner_remains > 0 {
        msg!("Transferring remains back to sender");
        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.sender_tokens.key,
                acc.escrow_tokens.key,
                &[],
                recipient_remains + streamflow_remains + partner_remains,
            )?,
            &[
                acc.escrow_tokens.clone(), // src
                acc.sender_tokens.clone(), // dest
                acc.escrow_tokens.clone(), // auth
                acc.token_program.clone(), // program
            ],
            &[&seeds],
        )?;
    }

    // TODO: Close metadata account once there is an alternative storage
    // solution for historical data.
    msg!("Closing escrow account");
    invoke_signed(
        &spl_token::instruction::close_account(
            acc.token_program.key,
            acc.escrow_tokens.key,
            acc.streamflow_treasury.key,
            acc.escrow_tokens.key,
            &[],
        )?,
        &[acc.escrow_tokens.clone(), acc.streamflow_treasury.clone(), acc.escrow_tokens.clone()],
        &[&seeds],
    )?;

    // TODO: What's with the if clause here?
    if now < metadata.closable_at {
        metadata.last_withdrawn_at = now;
        metadata.canceled_at = now;
    }

    // Write the metadata to the account
    let bytes = metadata.try_to_vec()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    Ok(())
}

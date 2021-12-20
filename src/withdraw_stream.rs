use std::str::FromStr;

use borsh::BorshSerialize;
use solana_program::{
    account_info::AccountInfo,
    borsh as solana_borsh,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    system_program, sysvar,
    sysvar::{clock::Clock, Sysvar},
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};

use crate::{
    error::SfError,
    state::{InstructionAccounts, TokenStreamData},
    utils::{calculate_available, encode_base10, unpack_mint_account},
    STRM_TREASURY,
};

#[derive(Clone, Debug)]
pub struct WithdrawAccounts<'a> {
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
    pub rent: AccountInfo<'a>,
    pub token_program: AccountInfo<'a>,
    pub associated_token_program: AccountInfo<'a>,
    pub system_program: AccountInfo<'a>,
}

fn account_sanity_check(program_id: &Pubkey, a: WithdrawAccounts) -> ProgramResult {
    msg!("Checking if all given accounts are correct");

    // These accounts must not be empty, and need to have correct ownership
    if a.escrow_tokens.data_is_empty() || a.escrow_tokens.owner != &spl_token::id() {
        return Err(SfError::InvalidEscrowAccount.into())
    }

    if a.metadata.data_is_empty() || a.metadata.owner != program_id {
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
    let escrow_tokens_pubkey =
        Pubkey::find_program_address(&[a.metadata.key.as_ref()], program_id).0;
    if &escrow_tokens_pubkey != a.escrow_tokens.key {
        return Err(ProgramError::InvalidAccountData)
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

fn metadata_sanity_check(acc: WithdrawAccounts, metadata: TokenStreamData) -> ProgramResult {
    let recipient_tokens_pubkey = get_associated_token_address(acc.recipient.key, acc.mint.key);
    let partner_tokens_pubkey = get_associated_token_address(acc.partner.key, acc.mint.key);
    let streamflow_treasury_tokens_pubkey =
        get_associated_token_address(acc.streamflow_treasury.key, acc.mint.key);

    if acc.recipient.key != &metadata.recipient ||
        acc.recipient_tokens.key != &recipient_tokens_pubkey ||
        acc.recipient_tokens.key != &metadata.recipient_tokens ||
        acc.mint.key != &metadata.mint ||
        acc.escrow_tokens.key != &metadata.escrow_tokens ||
        acc.streamflow_treasury.key != &metadata.streamflow_treasury ||
        acc.streamflow_treasury_tokens.key != &streamflow_treasury_tokens_pubkey ||
        acc.streamflow_treasury_tokens.key != &metadata.streamflow_treasury_tokens ||
        acc.partner.key != &metadata.partner ||
        acc.partner_tokens.key != &partner_tokens_pubkey ||
        acc.partner_tokens.key != &metadata.partner_tokens
    {
        return Err(SfError::MetadataAccountMismatch.into())
    }

    // TODO: What else?

    // Passed without touching the lasers
    Ok(())
}

/// Withdraw from an SPL Token stream
///
/// The function will read the instructions from the metadata account and see
/// if there are any unlocked funds. If so, they will be transferred from the
/// escrow account to the stream recipient.
pub fn withdraw(program_id: &Pubkey, acc: WithdrawAccounts, amount: u64) -> ProgramResult {
    msg!("Withdrawing from SPL token stream");

    let now = Clock::get()?.unix_timestamp as u64;
    let mint_info = unpack_mint_account(&acc.mint)?;

    account_sanity_check(program_id, acc.clone())?;

    let mut data = acc.metadata.try_borrow_mut_data()?;
    let mut metadata: TokenStreamData = match solana_borsh::try_from_slice_unchecked(&data) {
        Ok(v) => v,
        Err(_) => return Err(SfError::InvalidMetadata.into()),
    };

    metadata_sanity_check(acc.clone(), metadata.clone())?;

    // TODO: Check signer(s)

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

    let escrow_tokens_bump =
        Pubkey::find_program_address(&[acc.metadata.key.as_ref()], program_id).1;
    let seeds = [acc.metadata.key.as_ref(), &[escrow_tokens_bump]];

    if recipient_available > 0 {
        msg!("Transferring unlocked tokens to recipient");
        if acc.recipient_tokens.data_is_empty() {
            msg!("Initializing recipient's associated token account");
            invoke(
                &create_associated_token_account(acc.sender.key, acc.recipient.key, acc.mint.key),
                &[
                    acc.sender.clone(),
                    acc.recipient_tokens.clone(),
                    acc.recipient.clone(),
                    acc.mint.clone(),
                    acc.system_program.clone(),
                    acc.token_program.clone(),
                    acc.rent.clone(),
                ],
            )?;
        }

        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.recipient_tokens.key,
                acc.escrow_tokens.key,
                &[],
                recipient_available, // TODO: FIXME
            )?,
            &[
                acc.escrow_tokens.clone(),    // src
                acc.recipient_tokens.clone(), // dest
                acc.escrow_tokens.clone(),    // auth
                acc.token_program.clone(),    // program
            ],
            &[&seeds],
        )?;

        metadata.withdrawn_amount += recipient_available; // TODO: FIXME
        metadata.last_withdrawn_at = now;
        msg!(
            "Withdrawn: {} {} tokens",
            encode_base10(recipient_available, mint_info.decimals.into()),
            metadata.mint
        );
        msg!(
            "Remaining: {} {} tokens",
            encode_base10(
                metadata.ix.deposited_amount - metadata.withdrawn_amount,
                mint_info.decimals.into()
            ),
            metadata.mint
        );
    }

    if streamflow_available > 0 {
        msg!("Transferring unlocked tokens to Streamflow treasury");
        if acc.streamflow_treasury_tokens.data_is_empty() {
            msg!("Initializing Streamflow treasury associated token account");
            invoke(
                &create_associated_token_account(
                    acc.sender.key,
                    acc.streamflow_treasury.key,
                    acc.mint.key,
                ),
                &[
                    acc.sender.clone(),
                    acc.streamflow_treasury_tokens.clone(),
                    acc.streamflow_treasury.clone(),
                    acc.mint.clone(),
                    acc.system_program.clone(),
                    acc.token_program.clone(),
                    acc.rent.clone(),
                ],
            )?;
        }

        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.streamflow_treasury_tokens.key,
                acc.escrow_tokens.key,
                &[],
                streamflow_available, // TODO: FIXME
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
            encode_base10(streamflow_available, mint_info.decimals.into()),
            metadata.mint
        );
        msg!(
            "Remaining: {} {} tokens",
            encode_base10(
                metadata.streamflow_fee_total - metadata.streamflow_fee_withdrawn,
                mint_info.decimals.into()
            ),
            metadata.mint
        );
    }

    if partner_available > 0 {
        msg!("Transferring unlocked tokens to partner");
        if acc.partner_tokens.data_is_empty() {
            msg!("Initializing partner's associated token account");
            invoke(
                &create_associated_token_account(acc.sender.key, acc.partner.key, acc.mint.key),
                &[
                    acc.sender.clone(),
                    acc.partner_tokens.clone(),
                    acc.partner.clone(),
                    acc.mint.clone(),
                    acc.system_program.clone(),
                    acc.token_program.clone(),
                    acc.rent.clone(),
                ],
            )?;
        }

        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.partner_tokens.key,
                acc.escrow_tokens.key,
                &[],
                partner_available, // TODO: FIXME
            )?,
            &[
                acc.escrow_tokens.clone(),  // src
                acc.partner_tokens.clone(), // dest
                acc.escrow_tokens.clone(),  // auth
                acc.token_program.clone(),  // program
            ],
            &[&seeds],
        )?;

        metadata.partner_fee_withdrawn += partner_available; // TODO: FIXME
        metadata.last_withdrawn_at = now;
        msg!(
            "Withdrawn: {} {} tokens",
            encode_base10(partner_available, mint_info.decimals.into()),
            metadata.mint
        );
        msg!(
            "Remaining: {} {} tokens",
            encode_base10(
                metadata.partner_fee_total - metadata.partner_fee_withdrawn,
                mint_info.decimals.into()
            ),
            metadata.mint
        );
    }

    // Write the metadata to the account
    let bytes = metadata.try_to_vec()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    // When everything is withdrawn, close the accounts.
    // TODO: Should we really be comparing to deposited amount?
    if metadata.withdrawn_amount == metadata.ix.deposited_amount &&
        metadata.partner_fee_withdrawn == metadata.partner_fee_total &&
        metadata.streamflow_fee_withdrawn == metadata.streamflow_fee_total
    {
        // TODO: Close metadata account once there is an alternative storage solution
        // for historical data.
        // let rent = acc.metadata.lamports();
        // **acc.metadata.try_borrow_mut_lamports()? -= rent;
        // **acc.streamflow_treasury.try_borrow_mut_lamports()? += rent;

        invoke_signed(
            &spl_token::instruction::close_account(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.streamflow_treasury.key,
                acc.escrow_tokens.key,
                &[],
            )?,
            &[
                acc.escrow_tokens.clone(),
                acc.streamflow_treasury.clone(),
                acc.escrow_tokens.clone(),
            ],
            &[&seeds],
        )?;
    }

    Ok(())
}

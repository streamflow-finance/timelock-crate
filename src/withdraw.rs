use std::str::FromStr;

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
    process,
    state::{find_escrow_account, save_account_info, Contract, ESCROW_SEED_PREFIX, STRM_TREASURY},
    utils::{calculate_available, unpack_mint_account, unpack_token_account, Invoker},
};

#[derive(Clone, Debug)]
pub struct WithdrawAccounts<'a> {
    /// Account invoking transaction.
    pub authority: AccountInfo<'a>, // [writable, signer]
    /// Recipient's wallet address
    pub recipient: AccountInfo<'a>, // []
    /// The associated token account address of a stream `recipient`
    pub recipient_tokens: AccountInfo<'a>, // [writable]
    /// The account holding the stream parameters
    pub metadata: AccountInfo<'a>, // [writable]
    /// The escrow account holding the stream funds
    pub escrow_tokens: AccountInfo<'a>, // [writable]
    /// Streamflow treasury account
    pub streamflow_treasury: AccountInfo<'a>, // [writable]
    /// Streamflow treasury's associated token account
    pub streamflow_treasury_tokens: AccountInfo<'a>, // [writable]
    /// Partner treasury account
    pub partner: AccountInfo<'a>, // []
    /// Partner's associated token account
    pub partner_tokens: AccountInfo<'a>, // [writable]
    /// The SPL token mint account
    pub mint: AccountInfo<'a>, // []
    /// The SPL token program
    pub token_program: AccountInfo<'a>, // []
}

fn account_sanity_check(pid: &Pubkey, a: WithdrawAccounts) -> ProgramResult {
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
    let recipient_tokens = get_associated_token_address(a.recipient.key, a.mint.key);
    let partner_tokens = get_associated_token_address(a.partner.key, a.mint.key);

    if a.streamflow_treasury.key != &strm_treasury_pubkey ||
        a.streamflow_treasury_tokens.key != &strm_treasury_tokens
    {
        return Err(SfError::InvalidTreasury.into())
    }

    if a.recipient_tokens.key != &recipient_tokens || a.partner_tokens.key != &partner_tokens {
        return Err(ProgramError::InvalidAccountData)
    }

    // On-chain program ID checks
    if a.token_program.key != &spl_token::id() {
        return Err(ProgramError::InvalidAccountData)
    }

    // Passed without touching the lasers
    Ok(())
}

fn metadata_sanity_check(acc: WithdrawAccounts, metadata: Contract) -> ProgramResult {
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

    // Passed without touching the lasers
    Ok(())
}

/// Withdraw from an SPL Token stream
///
/// The function will read the instructions from the metadata account and see
/// if there are any unlocked funds. If so, they will be transferred from the
/// escrow account to the stream recipient.
pub fn withdraw(pid: &Pubkey, acc: WithdrawAccounts, amount: u64) -> ProgramResult {
    msg!("Withdrawing from SPL token stream");

    let now = Clock::get()?.unix_timestamp as u64;
    let mint_info = unpack_mint_account(&acc.mint)?;

    if !acc.authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature)
    }

    account_sanity_check(pid, acc.clone())?;

    let data = acc.metadata.try_borrow_mut_data()?;
    let mut metadata: Contract = match solana_borsh::try_from_slice_unchecked(&data) {
        Ok(v) => v,
        Err(_) => return Err(SfError::InvalidMetadata.into()),
    };

    // Taking the protocol version from the metadata, we check that the token
    // escrow account is correct:
    let (escrow_tokens_pubkey, escrow_tokens_bump) =
        find_escrow_account(metadata.version, acc.metadata.key.as_ref(), pid);
    if &escrow_tokens_pubkey != acc.escrow_tokens.key {
        return Err(ProgramError::InvalidAccountData)
    }

    metadata_sanity_check(acc.clone(), metadata.clone())?;

    // Confirm the signer is actually authorized for this instruction
    let withdraw_authority = Invoker::new(
        acc.authority.key,
        &metadata.sender,
        &metadata.recipient,
        &metadata.streamflow_treasury,
        &metadata.partner,
    );

    if !withdraw_authority.can_withdraw(metadata.ix.automatic_withdrawal, amount) {
        return Err(ProgramError::InvalidAccountData)
    }

    let escrow_tokens = unpack_token_account(&acc.escrow_tokens)?;
    if metadata.ix.can_topup {
        metadata.sync_balance(escrow_tokens.amount)
    }

    // Check what has been unlocked so far
    let recipient_available = calculate_available(
        now,
        metadata.end_time,
        metadata.ix.clone(),
        metadata.ix.net_amount_deposited,
        metadata.amount_withdrawn,
        100.0,
    );

    let streamflow_available = calculate_available(
        now,
        metadata.end_time,
        metadata.ix.clone(),
        metadata.streamflow_fee_total,
        metadata.streamflow_fee_withdrawn,
        metadata.streamflow_fee_percent,
    );

    let partner_available = calculate_available(
        now,
        metadata.end_time,
        metadata.ix.clone(),
        metadata.partner_fee_total,
        metadata.partner_fee_withdrawn,
        metadata.partner_fee_percent,
    );

    if amount > recipient_available {
        msg!("Available for recipient: {}", recipient_available);
        return Err(SfError::AmountMoreThanAvailable.into())
    }

    let seeds = [ESCROW_SEED_PREFIX, acc.metadata.key.as_ref(), &[escrow_tokens_bump]];

    if amount > 0 {
        msg!("Transferring unlocked tokens to recipient");
        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.recipient_tokens.key,
                acc.escrow_tokens.key,
                &[],
                amount,
            )?,
            &[
                acc.escrow_tokens.clone(),    // src
                acc.recipient_tokens.clone(), // dest
                acc.escrow_tokens.clone(),    // auth
                acc.token_program.clone(),    // program
            ],
            &[&seeds],
        )?;

        metadata.amount_withdrawn += amount;
        metadata.last_withdrawn_at = now;
        msg!(
            "Withdrawn: {} {} tokens",
            amount_to_ui_amount(amount, mint_info.decimals),
            metadata.mint
        );
        msg!(
            "Remaining: {} {} tokens",
            amount_to_ui_amount(
                metadata.ix.net_amount_deposited - metadata.amount_withdrawn,
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
                streamflow_available, //always max available
            )?,
            &[
                acc.escrow_tokens.clone(),              // src
                acc.streamflow_treasury_tokens.clone(), // dest
                acc.escrow_tokens.clone(),              // auth
                acc.token_program.clone(),              // program
            ],
            &[&seeds],
        )?;

        metadata.streamflow_fee_withdrawn += streamflow_available;
        msg!(
            "Withdrawn: {} {} tokens",
            amount_to_ui_amount(streamflow_available, mint_info.decimals),
            metadata.mint
        );
        msg!("Strm fee total: {}", metadata.streamflow_fee_total);
        msg!("Strm fee withdrawn: {}", metadata.streamflow_fee_withdrawn);
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
                partner_available, //always max available for partner
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

    save_account_info(&metadata, data)?;

    // When everything is withdrawn, close the accounts.
    if now >= metadata.end_time {
        // TODO: Close metadata account once there is an alternative storage solution
        // for historical data.
        // msg!("Closing metadata account");
        // let rent = acc.metadata.lamports();
        // **acc.metadata.try_borrow_mut_lamports()? -= rent;
        // **acc.streamflow_treasury.try_borrow_mut_lamports()? += rent;

        process::close_escrow(
            &metadata,
            &seeds,
            acc.token_program,
            acc.escrow_tokens,
            acc.streamflow_treasury,
            acc.streamflow_treasury_tokens,
        )?;
    }

    Ok(())
}

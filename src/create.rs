use std::{cmp::max, str::FromStr};

use borsh::BorshSerialize;
use partner_oracle::fees::fetch_partner_fee_data;
use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    system_instruction, system_program, sysvar,
    sysvar::{clock::Clock, rent::Rent, Sysvar},
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::amount_to_ui_amount;

use crate::{
    error::SfError,
    state::{
        find_escrow_account, save_account_info, Contract, CreateParams, ESCROW_SEED_PREFIX,
        PROGRAM_VERSION, STRM_FEE_DEFAULT_PERCENT, STRM_TREASURY,
    },
    utils::{
        calculate_fee_from_amount, duration_sanity, pretty_time, unpack_mint_account,
        unpack_token_account,
    },
};

#[derive(Clone, Debug)]
// TODO: Add liquidator account (think of a better name for it too)
pub struct CreateAccounts<'a> {
    /// Wallet of the stream creator.
    pub sender: AccountInfo<'a>, // [writable, signer]
    /// Associated token account address of `sender`.
    pub sender_tokens: AccountInfo<'a>, // [writable]
    /// Wallet address of the recipient.
    pub recipient: AccountInfo<'a>, // []
    /// Associated token account address of `recipient`.
    pub recipient_tokens: AccountInfo<'a>, // [writable]
    /// The account holding the stream parameters.
    /// Expects empty (non-initialized) account.
    pub metadata: AccountInfo<'a>, // [writable, signer]
    /// The escrow account holding the funds.
    /// Expects empty (non-initialized) account.
    pub escrow_tokens: AccountInfo<'a>, // [writable]
    /// Streamflow treasury account
    pub streamflow_treasury: AccountInfo<'a>, // []
    /// Streamflow treasury's associated token account
    pub streamflow_treasury_tokens: AccountInfo<'a>, // [writable]
    /// Partner treasury account
    pub partner: AccountInfo<'a>, // []
    /// Partner's associated token account
    pub partner_tokens: AccountInfo<'a>, // [writable]
    /// The SPL token mint account
    pub mint: AccountInfo<'a>, // []
    /// Internal program that handles fees for specified partners
    pub fee_oracle: AccountInfo<'a>, // []
    /// The Rent Sysvar account
    pub rent: AccountInfo<'a>, // []
    /// The SPL program needed in case an associated account
    /// for the new recipient is being created.
    pub token_program: AccountInfo<'a>, // []
    /// The Associated Token program needed in case associated
    /// account for the new recipient is being created.
    pub associated_token_program: AccountInfo<'a>, // []
    /// The Solana system program needed for account creation
    pub system_program: AccountInfo<'a>, // []
}

fn account_sanity_check(pid: &Pubkey, a: CreateAccounts) -> ProgramResult {
    msg!("Checking if all given accounts are correct");

    // We want these to not be initialized
    if !a.escrow_tokens.data_is_empty() || !a.metadata.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized)
    }

    // We want these accounts to be writable
    if !a.sender.is_writable ||             //fee payer
        !a.sender_tokens.is_writable ||     //debtor
        !a.recipient_tokens.is_writable ||  //might be created
        !a.metadata.is_writable ||          //will be created
        !a.escrow_tokens.is_writable ||     //creditor
        !a.streamflow_treasury_tokens.is_writable || //might be created
        !a.partner_tokens.is_writable
    //might be created
    // || !a.liquidator.is_writable //creditor (tx fees)
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
        return Err(SfError::NotAssociated.into())
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

fn instruction_sanity_check(ix: CreateParams, now: u64) -> ProgramResult {
    // Check if timestamps are all in order and valid
    duration_sanity(now, ix.start_time, ix.cliff)?;

    // Can't deposit less than what's needed for one period
    if ix.net_amount_deposited < ix.amount_per_period {
        return Err(SfError::InvalidDeposit.into())
    }

    if ix.cliff_amount > 0 && ix.net_amount_deposited < ix.cliff_amount {
        return Err(SfError::InvalidDeposit.into())
    }

    // Passed without touching the lasers.
    Ok(())
}

pub fn create(pid: &Pubkey, acc: CreateAccounts, ix: CreateParams) -> ProgramResult {
    msg!("Initializing SPL token stream");

    // The stream initializer, and the keypair for creating the metadata account must sign this.
    if !acc.sender.is_signer || !acc.metadata.is_signer {
        return Err(ProgramError::MissingRequiredSignature)
    }

    let mint_info = unpack_mint_account(&acc.mint)?;
    let now = Clock::get()?.unix_timestamp as u64;

    // Sanity checks
    account_sanity_check(pid, acc.clone())?;
    instruction_sanity_check(ix.clone(), now)?;

    // Check escrow token account is legit
    let (escrow_tokens_pubkey, stream_escrow_bump) =
        find_escrow_account(PROGRAM_VERSION, acc.metadata.key.as_ref(), pid);
    if &escrow_tokens_pubkey != acc.escrow_tokens.key {
        return Err(ProgramError::InvalidAccountData)
    }

    // Check partner accounts are legit
    let (mut partner_fee_percent, mut strm_fee_percent) = (0.0, STRM_FEE_DEFAULT_PERCENT);
    //TODO: unlock once deployed.
    // match fetch_partner_fee_data(&acc.fee_oracle, acc.partner.key) {
    //     Ok(v) => v,
    //     // In case the partner is not found, we fallback to default.
    //     Err(_) => (0.0, STRM_FEE_DEFAULT_PERCENT),
    // };

    partner_fee_percent = max(partner_fee_percent, 0.5); //this way we ensure that fee can't be larger than 0.5%
    strm_fee_percent = max(strm_fee_percent, 0.5); //this way we ensure that fee can't be larger than 0.5%

    // Calculate fees
    let partner_fee_amount =
        calculate_fee_from_amount(ix.net_amount_deposited, partner_fee_percent);
    let strm_fee_amount = calculate_fee_from_amount(ix.net_amount_deposited, strm_fee_percent);
    msg!("Partner fee: {}", amount_to_ui_amount(partner_fee_amount, mint_info.decimals));
    msg!("Streamflow fee: {}", amount_to_ui_amount(strm_fee_amount, mint_info.decimals));

    let gross_amount = ix.net_amount_deposited + partner_fee_amount + strm_fee_amount;

    let sender_tokens = unpack_token_account(&acc.sender_tokens)?;
    if sender_tokens.amount < gross_amount {
        return Err(ProgramError::InsufficientFunds)
    }

    let mut metadata = Contract::new(
        now,
        acc.clone(),
        ix.clone(),
        partner_fee_amount,
        partner_fee_percent,
        strm_fee_amount,
        strm_fee_percent,
    );

    let metadata_bytes = metadata.try_to_vec()?;
    // We pad % 8 for size , since that's what has to be allocated.
    let mut metadata_struct_size = metadata_bytes.len();
    while metadata_struct_size % 8 > 0 {
        metadata_struct_size += 1;
    }
    let tokens_struct_size = spl_token::state::Account::LEN;

    let cluster_rent = Rent::get()?;
    let metadata_rent = cluster_rent.minimum_balance(metadata_struct_size);
    let mut tokens_rent = cluster_rent.minimum_balance(tokens_struct_size);
    if acc.recipient_tokens.data_is_empty() {
        tokens_rent *= cluster_rent.minimum_balance(tokens_struct_size);
    }

    if acc.sender.lamports() < metadata_rent + tokens_rent {
        msg!("Error: Insufficient funds in {}", acc.sender.key);
        return Err(ProgramError::InsufficientFunds)
    }

    msg!("Creating stream metadata account");
    invoke(
        &system_instruction::create_account(
            acc.sender.key,
            acc.metadata.key,
            metadata_rent,
            metadata_struct_size as u64,
            pid,
        ),
        &[acc.sender.clone(), acc.metadata.clone(), acc.system_program.clone()],
    )?;

    msg!("Writing metadata into the account");
    let data = acc.metadata.try_borrow_mut_data()?;
    save_account_info(&metadata, data)?;

    msg!("Creating stream escrow account");
    let seeds = [ESCROW_SEED_PREFIX, acc.metadata.key.as_ref(), &[stream_escrow_bump]];

    invoke_signed(
        &system_instruction::create_account(
            acc.sender.key,
            acc.escrow_tokens.key,
            tokens_rent,
            tokens_struct_size as u64,
            &spl_token::id(),
        ),
        &[acc.sender.clone(), acc.escrow_tokens.clone(), acc.system_program.clone()],
        &[&seeds],
    )?;

    msg!("Initializing stream escrow SPL token account ");
    invoke(
        &spl_token::instruction::initialize_account(
            acc.token_program.key,
            acc.escrow_tokens.key,
            acc.mint.key,
            acc.escrow_tokens.key,
        )?,
        &[
            acc.token_program.clone(),
            acc.escrow_tokens.clone(),
            acc.mint.clone(),
            acc.escrow_tokens.clone(), // owner
            acc.rent.clone(),
        ],
    )?;

    msg!("Moving funds into escrow");
    invoke(
        &spl_token::instruction::transfer(
            acc.token_program.key,
            acc.sender_tokens.key,
            acc.escrow_tokens.key,
            acc.sender.key,
            &[],
            gross_amount,
        )?,
        &[
            acc.sender_tokens.clone(),
            acc.escrow_tokens.clone(),
            acc.sender.clone(),
            acc.token_program.clone(),
        ],
    )?;

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

    if partner_fee_percent > 0.0 && acc.partner_tokens.data_is_empty() {
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

    if strm_fee_percent > 0.0 && acc.streamflow_treasury_tokens.data_is_empty() {
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

    msg!(
        "Success initializing {} {} token_stream for {}",
        amount_to_ui_amount(ix.net_amount_deposited, mint_info.decimals),
        acc.mint.key,
        acc.recipient.key
    );

    msg!("Called by {}", acc.sender.key);
    msg!("Metadata written in {}", acc.metadata.key);
    msg!("Funds locked in {}", acc.escrow_tokens.key);
    msg!("Stream duration is {}", pretty_time(metadata.end_time - ix.start_time));

    if ix.cliff > 0 && ix.cliff_amount > 0 {
        msg!("Cliff happens in {}", pretty_time(ix.cliff));
    }

    Ok(())
}

use std::str::FromStr;

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
    state::{Contract, CreateParams, MAX_STRING_SIZE, STRM_FEE_DEFAULT_PERCENT, STRM_TREASURY},
    utils::{calculate_fee_from_amount, duration_sanity, format, pretty_time, unpack_mint_account},
};

#[derive(Clone, Debug)]
pub struct CreateAccounts<'a> {
    /// Wallet of the stream creator.
    pub sender: AccountInfo<'a>,
    /// Associated token account address of `sender`.
    pub sender_tokens: AccountInfo<'a>,
    /// Wallet address of the recipient.
    pub recipient: AccountInfo<'a>,
    /// Associated token account address of `recipient`.
    pub recipient_tokens: AccountInfo<'a>,
    /// The account holding the stream parameters.
    /// Expects empty (non-initialized) account.
    pub metadata: AccountInfo<'a>,
    /// The escrow account holding the funds.
    /// Expects empty (non-initialized) account.
    pub escrow_tokens: AccountInfo<'a>,
    /// Streamflow treasury account
    pub streamflow_treasury: AccountInfo<'a>,
    /// Streamflow treasury's associated token account
    pub streamflow_treasury_tokens: AccountInfo<'a>,
    /// Partner treasury account
    pub partner: AccountInfo<'a>,
    /// Partner's associated token account
    pub partner_tokens: AccountInfo<'a>,
    /// The SPL token mint account
    pub mint: AccountInfo<'a>,
    /// Internal program that handles fees for specified partners
    pub fee_oracle: AccountInfo<'a>,
    /// The Rent Sysvar account
    pub rent: AccountInfo<'a>,
    /// The SPL program needed in case an associated account
    /// for the new recipient is being created.
    pub token_program: AccountInfo<'a>,
    /// The Associated Token program needed in case associated
    /// account for the new recipient is being created.
    pub associated_token_program: AccountInfo<'a>,
    /// The Solana system program needed for account creation
    pub system_program: AccountInfo<'a>,
    //todo: add "liquidator" (think of better name, too)
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

    // Check escrow token account is legit
    // TODO: Needs a deterministic seed and metadata should become a PDA
    let escrow_tokens_pubkey = Pubkey::find_program_address(&[a.metadata.key.as_ref()], pid).0;
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

fn instruction_sanity_check(ix: CreateParams, now: u64) -> ProgramResult {
    // We'll limit the stream name length
    if ix.stream_name.len() > MAX_STRING_SIZE {
        return Err(SfError::StreamNameTooLong.into())
    }

    // Check if timestamps are all in order and valid
    duration_sanity(now, ix.start_time, ix.end_time, ix.cliff)?;

    // Can't deposit less than what's needed for one period
    if ix.amount_deposited < ix.amount_per_period {
        return Err(SfError::InvalidDeposit.into())
    }

    // TODO: We have 2 conflicting parameter fields:
    // Check how contract.amount_per_period vibes with
    // num_periods = (end - cliff) / period;
    // amount_per_period = amount_deposited / num_periods
    // i.e.
    // - if we set the end date, then release rate is calculated based on the end date
    // - if we set the release rate, then the end date is calculated based on this
    //TODO: Solution: input only release rate and calculate end_date based upon that.

    // TODO: Anything else?

    // Passed without touching the lasers.
    Ok(())
}

pub fn create(pid: &Pubkey, acc: CreateAccounts, ix: CreateParams) -> ProgramResult {
    msg!("Initializing SPL token stream");

    // The stream initializer, and the keypair for creating the metadata account must sign this.
    // TODO: Metadata should be a PDA
    if !acc.sender.is_signer || !acc.metadata.is_signer {
        return Err(ProgramError::MissingRequiredSignature)
    }

    let cluster_rent = Rent::get()?;
    let mint_info = unpack_mint_account(&acc.mint)?;
    let now = Clock::get()?.unix_timestamp as u64;

    // Sanity checks
    account_sanity_check(pid, acc.clone())?;
    instruction_sanity_check(ix.clone(), now)?;

    // TODO: Check available balances?

    // Check partner accounts are legit
    // TODO: How to enforce correct partner account?
    //Todo: can we do a CPI (invoke) here to further obfuscate internal structure of fees account?
    let (partner_fee_percent, strm_fee_percent) =
        match fetch_partner_fee_data(&acc.fee_oracle, acc.partner.key) {
            Ok(v) => v,
            // In case the partner is not found, we fallback to default.
            Err(_) => (0.0, STRM_FEE_DEFAULT_PERCENT),
        };

    // Calculate fees
    let partner_fee_amount = calculate_fee_from_amount(ix.amount_deposited, partner_fee_percent);
    let strm_fee_amount = calculate_fee_from_amount(ix.amount_deposited, strm_fee_percent);
    msg!("Partner fee: {}", format(partner_fee_amount, mint_info.decimals as usize));
    msg!("Streamflow fee: {}", format(strm_fee_amount, mint_info.decimals as usize));

    let mut metadata = Contract::new(
        now,
        acc.clone(),
        ix.clone(),
        partner_fee_amount,
        partner_fee_percent,
        strm_fee_amount,
        strm_fee_percent,
    );

    // Move closable_at (from third party), when recurring ignore end_date
    if ix.release_rate > 0 {
        metadata.closable_at = metadata.closable();
        msg!("Closable at: {}", metadata.closable_at);
    }

    let metadata_bytes = metadata.try_to_vec()?;
    let mut metadata_struct_size = metadata_bytes.len();
    // We pad % 8 for size, since that's what has to be allocated
    while metadata_struct_size % 8 > 0 {
        metadata_struct_size += 1;
    }

    msg!("Creating stream metadata account");
    invoke(
        &system_instruction::create_account(
            acc.sender.key,
            acc.metadata.key,
            cluster_rent.minimum_balance(metadata_struct_size),
            metadata_struct_size as u64,
            pid,
        ),
        &[acc.sender.clone(), acc.metadata.clone(), acc.system_program.clone()],
    )?;

    msg!("Writing metadata into the account");
    let mut data = acc.metadata.try_borrow_mut_data()?;
    data[0..metadata_bytes.len()].clone_from_slice(&metadata_bytes);

    msg!("Creating stream escrow account");
    // TODO: This seed should be deterministic and metadata should be PDA
    let stream_escrow_bump = Pubkey::find_program_address(&[acc.metadata.key.as_ref()], pid).1;
    let seeds = [acc.metadata.key.as_ref(), &[stream_escrow_bump]];

    invoke_signed(
        &system_instruction::create_account(
            acc.sender.key,
            acc.escrow_tokens.key,
            cluster_rent.minimum_balance(spl_token::state::Account::LEN),
            spl_token::state::Account::LEN as u64,
            &spl_token::id(),
        ),
        &[acc.sender.clone(), acc.escrow_tokens.clone(), acc.system_program.clone()],
        &[&seeds],
    )?;

    msg!("Initializing stream escrow account for SPL token");
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
            ix.amount_deposited + partner_fee_amount + strm_fee_amount,
        )?,
        &[
            acc.sender_tokens.clone(),
            acc.escrow_tokens.clone(),
            acc.sender.clone(),
            acc.token_program.clone(),
        ],
    )?;

    // TODO: Check unpack_token_account for ATA if we decide they shouldn't be initialized
    // (all around the codebase)
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
        msg!("Initializing parther's associated token account");
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
        amount_to_ui_amount(ix.amount_deposited, mint_info.decimals),
        acc.mint.key,
        acc.recipient.key
    );

    msg!("Called by {}", acc.sender.key);
    msg!("Metadata written in {}", acc.metadata.key);
    msg!("Funds locked in {}", acc.escrow_tokens.key);
    msg!("Stream duration is {}", pretty_time(ix.end_time - ix.start_time));

    if ix.cliff > 0 && ix.cliff_amount > 0 {
        msg!("Cliff happens in {}", pretty_time(ix.cliff));
    }

    Ok(())
}

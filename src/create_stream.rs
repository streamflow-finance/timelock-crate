use borsh::BorshSerialize;
use num_traits::cast::FromPrimitive;
use partner_oracle::fees::fetch_partner_fee_data;
use solana_program::{
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    system_instruction,
    sysvar::{clock::Clock, rent::Rent, Sysvar},
};
use spl_associated_token_account::create_associated_token_account;

use crate::{
    error::SfError,
    state::{InstructionAccounts, StreamInstruction, TokenStreamData},
    stream_safety::uninitialized_account_sanity_check,
    utils::{duration_sanity, encode_base10, pretty_time, unpack_mint_account},
    MAX_STRING_SIZE,
};

fn instruction_sanity_check(ix: StreamInstruction, now: u64) -> ProgramResult {
    // We'll limit the stream name lenggth
    if ix.stream_name.len() > MAX_STRING_SIZE {
        return Err(SfError::StreamNameTooLong.into())
    }

    // Check if timestamps are all in order and valid
    duration_sanity(now, ix.start_time, ix.end_time, ix.cliff)?;

    if ix.deposited_amount > ix.total_amount {
        return Err(SfError::InvalidDeposit.into())
    }

    // TODO: Anything else?

    // Passed without touching the lasers.
    Ok(())
}

pub(crate) fn create(
    program_id: &Pubkey,
    acc: InstructionAccounts,
    ix: StreamInstruction,
) -> ProgramResult {
    msg!("Initializing SPL token stream");

    if !acc.sender.is_signer || !acc.metadata.is_signer {
        return Err(ProgramError::MissingRequiredSignature)
    }

    let cluster_rent = Rent::get()?;
    let now = Clock::get()?.unix_timestamp as u64;
    let mint_info = unpack_mint_account(&acc.mint)?;

    // Sanity checks
    uninitialized_account_sanity_check(program_id, acc.clone())?;
    instruction_sanity_check(ix.clone(), now)?;
    // TODO: Check available balances?

    // Check partner accounts are legit
    // TODO: How to enforce correct partner account?
    let (partner_fee, strm_fee) = match fetch_partner_fee_data(&acc.partner, acc.partner.key) {
        Ok(v) => v,
        // In case the partner is not found, we fallback to Streamflow.
        Err(_) => fetch_partner_fee_data(&acc.streamflow_treasury, acc.streamflow_treasury.key)?,
    };

    // Calculate fees
    let mut uint_fee_for_partner: u64 = 0;
    if partner_fee > 0.0 {
        // TODO: Test units
        let fee_for_partner = ix.deposited_amount as f64 * (partner_fee / 100.0) as f64;
        msg!("Fee for partner: {}", fee_for_partner);
        let r = fee_for_partner * f64::from_u8(mint_info.decimals).unwrap().floor();
        uint_fee_for_partner = r as u64;
    }

    let mut uint_fee_for_strm: u64 = 0;
    if strm_fee > 0.0 {
        // TODO: Test units
        let fee_for_strm = ix.deposited_amount as f64 * (strm_fee / 100.0) as f64;
        msg!("Fee for Streamflow: {}", fee_for_strm);
        let r = fee_for_strm * f64::from_u8(mint_info.decimals).unwrap().floor();
        uint_fee_for_strm = r as u64;
    }

    let mut metadata =
        TokenStreamData::new(now, acc.clone(), ix.clone(), uint_fee_for_partner, uint_fee_for_strm);
    // Move closable_at (from third party), when recurring ignore end_date
    if ix.deposited_amount < ix.total_amount || ix.release_rate > 0 {
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
            program_id,
        ),
        &[acc.sender.clone(), acc.metadata.clone(), acc.system_program.clone()],
    )?;

    msg!("Writing metadata into the account");
    let mut data = acc.metadata.try_borrow_mut_data()?;
    data[0..metadata_bytes.len()].clone_from_slice(&metadata_bytes);

    msg!("Creating stream escrow account");
    // TODO: This seed should be deterministic and metadata should be PDA
    let stream_escrow_bump =
        Pubkey::find_program_address(&[acc.metadata.key.as_ref()], program_id).1;
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
            ix.deposited_amount + uint_fee_for_partner + uint_fee_for_strm,
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

    if partner_fee > 0.0 && acc.partner_tokens.data_is_empty() {
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

    if strm_fee > 0.0 && acc.streamflow_treasury_tokens.data_is_empty() {
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
        encode_base10(ix.deposited_amount, mint_info.decimals.into()),
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

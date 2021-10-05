// Copyright (c) 2021 Ivan Jelincic <parazyd@dyne.org>
//
// This file is part of streamflow-timelock
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License version 3
// as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.
use solana_program::{
    entrypoint::ProgramResult,
    msg,
    native_token::lamports_to_sol,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    system_instruction, system_program, sysvar,
    sysvar::{clock::Clock, fees::Fees, rent::Rent, Sysvar},
};
use spl_associated_token_account::create_associated_token_account;

use crate::state::{
    CancelAccounts, InitializeAccounts, StreamInstruction, TokenStreamData, WithdrawAccounts,
};
use crate::utils::{
    duration_sanity, encode_base10, pretty_time, unpack_mint_account, unpack_token_account,
};

/// Initialize an SPL token stream
///
/// The function shall initialize new accounts to hold the tokens,
/// and the stream's metadata. Both accounts will be funded to be
/// rent-exempt if necessary. When the stream is finished, these
/// shall be returned to the stream initializer.
pub fn initialize_token_stream(
    program_id: &Pubkey,
    acc: InitializeAccounts,
    ix: StreamInstruction,
) -> ProgramResult {
    msg!("Initializing SPL token stream");

    if !acc.escrow_account.data_is_empty() || !acc.metadata_account.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    if !acc.sender_wallet.is_writable
        || !acc.sender_tokens.is_writable
        || !acc.recipient_wallet.is_writable//todo: could it be read-only?
        || !acc.recipient_tokens.is_writable
        || !acc.metadata_account.is_writable
        || !acc.escrow_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let (escrow_pubkey, nonce) =
        Pubkey::find_program_address(&[acc.metadata_account.key.as_ref()], program_id);

    if acc.system_program_account.key != &system_program::id()
        || acc.token_program_account.key != &spl_token::id()
        || acc.timelock_program_account.key != program_id
        || acc.rent_account.key != &sysvar::rent::id()
        || acc.escrow_account.key != &escrow_pubkey
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !acc.sender_wallet.is_signer || !acc.metadata_account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let sender_token_info = unpack_token_account(&acc.sender_tokens)?;
    let mint_info = unpack_mint_account(&acc.mint_account)?;

    if &sender_token_info.mint != acc.mint_account.key {
        return Err(ProgramError::InvalidAccountData);
    }

    let now = Clock::get()?.unix_timestamp as u64;
    if !duration_sanity(now, ix.start_time, ix.end_time, ix.cliff) {
        msg!("Error: Given timestamps are invalid");
        return Err(ProgramError::InvalidArgument);
    }

    // We also transfer enough to be rent-exempt on the metadata account.
    let metadata_struct_size = std::mem::size_of::<TokenStreamData>();
    let tokens_struct_size = spl_token::state::Account::LEN;
    let cluster_rent = Rent::get()?;
    let metadata_rent = cluster_rent.minimum_balance(metadata_struct_size);
    let mut tokens_rent = cluster_rent.minimum_balance(tokens_struct_size);
    let fees = Fees::get()?;
    let lps = fees.fee_calculator.lamports_per_signature;

    // Check if we have to initialize recipient's associated token account.
    if acc.recipient_tokens.data_is_empty() {
        tokens_rent += cluster_rent.minimum_balance(tokens_struct_size);
    }

    // TODO: Check if wrapped SOL
    if acc.sender_wallet.lamports() < metadata_rent + tokens_rent + (2 * lps) { //two signatures
        msg!("Error: Insufficient funds in {}", acc.sender_wallet.key);
        return Err(ProgramError::InsufficientFunds);
    }

    if sender_token_info.amount < ix.amount {
        msg!("Error: Insufficient tokens in sender's wallet");
        return Err(ProgramError::InsufficientFunds);
    }

    let metadata = TokenStreamData::new(
        ix.start_time,
        ix.end_time,
        ix.amount,
        ix.period,
        ix.cliff,
        ix.cliff_amount,
        *acc.sender_wallet.key,
        *acc.sender_tokens.key,
        *acc.recipient_wallet.key,
        *acc.recipient_tokens.key,
        *acc.mint_account.key,
        *acc.escrow_account.key,
    );
    let bytes = bincode::serialize(&metadata).unwrap();

    if acc.recipient_tokens.data_is_empty() {
        msg!("Initializing recipient's associated token account");
        invoke(
            &create_associated_token_account(
                acc.sender_wallet.key,
                acc.recipient_wallet.key,
                acc.mint_account.key,
            ),
            &[
                acc.sender_wallet.clone(),
                acc.recipient_tokens.clone(),
                acc.recipient_wallet.clone(),
                acc.mint_account.clone(),
                acc.system_program_account.clone(),
                acc.token_program_account.clone(),
                acc.rent_account.clone(),
            ],
        )?;
    }

    msg!("Creating account for holding metadata");
    invoke(
        &system_instruction::create_account(
            acc.sender_wallet.key,
            acc.metadata_account.key,
            metadata_rent,
            metadata_struct_size as u64,
            program_id,
        ),
        &[
            acc.sender_wallet.clone(),
            acc.metadata_account.clone(),
            acc.system_program_account.clone(),
        ],
    )?;

    // Write the metadata to the account
    let mut data = acc.metadata_account.try_borrow_mut_data()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    let seeds = [acc.metadata_account.key.as_ref(), &[nonce]];
    msg!("Creating account for holding tokens");
    invoke_signed(
        &system_instruction::create_account(
            acc.sender_wallet.key,
            acc.escrow_account.key,
            tokens_rent,
            tokens_struct_size as u64,
            &spl_token::id(),
        ),
        &[
            acc.sender_wallet.clone(),
            acc.escrow_account.clone(),
            acc.system_program_account.clone(),
        ],
        &[&seeds],
    )?;

    msg!(
        "Initializing escrow account for {} token",
        acc.mint_account.key
    );
    invoke(
        &spl_token::instruction::initialize_account(
            acc.token_program_account.key,
            acc.escrow_account.key,
            acc.mint_account.key,
            acc.escrow_account.key,
        )?,
        &[
            acc.token_program_account.clone(),
            acc.escrow_account.clone(),
            acc.mint_account.clone(),
            acc.escrow_account.clone(),
            acc.rent_account.clone(),
        ],
    )?;

    msg!("Moving funds into escrow account");
    invoke(
        &spl_token::instruction::transfer(
            acc.token_program_account.key,
            acc.sender_tokens.key,
            acc.escrow_account.key,
            acc.sender_wallet.key,
            &[],
            metadata.ix.amount,
        )?,
        &[
            acc.sender_tokens.clone(),
            acc.escrow_account.clone(),
            acc.sender_wallet.clone(),
            acc.token_program_account.clone(),
        ],
    )?;

    msg!(
        "Successfully initialized {} {} token stream for {}",
        encode_base10(metadata.ix.amount, mint_info.decimals.into()),
        metadata.mint,
        acc.recipient_wallet.key
    );
    msg!("Called by {}", acc.sender_wallet.key);
    msg!("Metadata written in {}", acc.metadata_account.key);
    msg!("Funds locked in {}", acc.escrow_account.key);
    msg!(
        "Stream duration is {}",
        pretty_time(metadata.ix.end_time - metadata.ix.start_time)
    );

    if metadata.ix.cliff > 0 && metadata.ix.cliff_amount > 0 {
        msg!("Cliff happens at {}", pretty_time(metadata.ix.cliff));
    }

    Ok(())
}

/// Withdraw from an SPL Token stream
///
/// The function will read the instructions from the metadata account and see
/// if there are any unlocked funds. If so, they will be transferred from the
/// escrow account to the stream recipient. If the entire amount has been
/// withdrawn, the remaining rents shall be returned to the stream initializer.
pub fn withdraw_token_stream(
    program_id: &Pubkey,
    acc: WithdrawAccounts,
    amount: u64,
) -> ProgramResult {
    msg!("Withdrawing from SPL token stream");

    if acc.escrow_account.data_is_empty()
        || acc.escrow_account.owner != &spl_token::id()
        || acc.metadata_account.data_is_empty()
        || acc.metadata_account.owner != program_id
    {
        return Err(ProgramError::UninitializedAccount);
    }

    if !acc.sender_wallet.is_writable
        || !acc.sender_tokens.is_writable
        || !acc.recipient_wallet.is_writable
        || !acc.recipient_tokens.is_writable
        || !acc.metadata_account.is_writable
        || !acc.escrow_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let (escrow_pubkey, nonce) =
        Pubkey::find_program_address(&[acc.metadata_account.key.as_ref()], program_id);

    if acc.system_program_account.key != &system_program::id()
        || acc.token_program_account.key != &spl_token::id()
        || acc.escrow_account.key != &escrow_pubkey
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !acc.recipient_wallet.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut data = acc.metadata_account.try_borrow_mut_data()?;
    let mut metadata = match bincode::deserialize::<TokenStreamData>(&data) {
        Ok(v) => v,
        Err(_) => return Err(ProgramError::Custom(143)),
    };

    let mint_info = unpack_mint_account(&acc.mint_account)?;

    if acc.sender_wallet.key != &metadata.sender_wallet
        || acc.sender_tokens.key != &metadata.sender_tokens
        || acc.recipient_wallet.key != &metadata.recipient_wallet
        || acc.recipient_tokens.key != &metadata.recipient_tokens
        || acc.mint_account.key != &metadata.mint
        || acc.escrow_account.key != &metadata.escrow
    {
        msg!("Error: Metadata does not match given accounts");
        return Err(ProgramError::InvalidAccountData);
    }

    let now = Clock::get()?.unix_timestamp as u64;
    let available = metadata.available(now);
    let req: u64;

    if amount > available {
        msg!("Amount requested for withdraw is more than what is available");
        return Err(ProgramError::InvalidArgument);
    }

    if amount == 0 {
        req = available;
    } else {
        req = amount;
    }

    let seeds = [acc.metadata_account.key.as_ref(), &[nonce]];

    invoke_signed(
        &spl_token::instruction::transfer(
            acc.token_program_account.key,
            acc.escrow_account.key,
            acc.recipient_tokens.key,
            acc.escrow_account.key,
            &[],
            req,
        )?,
        &[
            acc.escrow_account.clone(),
            acc.recipient_tokens.clone(),
            acc.escrow_account.clone(),
            acc.token_program_account.clone(),
        ],
        &[&seeds],
    )?;

    metadata.withdrawn += req;

    let bytes = bincode::serialize(&metadata).unwrap();
    data[0..bytes.len()].clone_from_slice(&bytes);

    // // Return rent when everything is withdrawn
    // if metadata.withdrawn == metadata.ix.amount {
    //     msg!("Returning rent to {}", acc.sender_wallet.key);
    //     let rent = acc.metadata_account.lamports();
    //     **acc.metadata_account.try_borrow_mut_lamports()? -= rent;
    //     **acc.sender_wallet.try_borrow_mut_lamports()? += rent;
    //
    //     // TODO: Close token account, has to have close authority
    // }

    msg!(
        "Withdrawn: {} {} tokens",
        encode_base10(req, mint_info.decimals.into()),
        metadata.mint
    );
    msg!(
        "Remaining: {} {} tokens",
        encode_base10(
            metadata.ix.amount - metadata.withdrawn,
            mint_info.decimals.into()
        ),
        metadata.mint
    );

    Ok(())
}

/// Cancel an SPL Token stream
///
/// The function will read the instructions from the metadata account and see
/// if there are any unlocked funds. If so, they will be transferred to the
/// stream recipient, and any remains (including rents) shall be returned to
/// the stream initializer.
pub fn cancel_token_stream(program_id: &Pubkey, acc: CancelAccounts) -> ProgramResult {
    msg!("Cancelling SPL token stream");

    if acc.escrow_account.data_is_empty()
        || acc.escrow_account.owner != &spl_token::id()
        || acc.metadata_account.data_is_empty()
        || acc.metadata_account.owner != program_id
    {
        return Err(ProgramError::UninitializedAccount);
    }

    if !acc.sender_wallet.is_writable
        || !acc.sender_tokens.is_writable
        || !acc.recipient_wallet.is_writable
        || !acc.recipient_tokens.is_writable
        || !acc.metadata_account.is_writable
        || !acc.escrow_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let (escrow_pubkey, nonce) =
        Pubkey::find_program_address(&[acc.metadata_account.key.as_ref()], program_id);

    if acc.system_program_account.key != &system_program::id()
        || acc.token_program_account.key != &spl_token::id()
        || acc.escrow_account.key != &escrow_pubkey
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !acc.sender_wallet.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let data = acc.metadata_account.try_borrow_mut_data()?;
    let mut metadata = match bincode::deserialize::<TokenStreamData>(&data) {
        Ok(v) => v,
        Err(_) => return Err(ProgramError::Custom(143)),
    };

    let mint_info = unpack_mint_account(&acc.mint_account)?;

    if acc.sender_wallet.key != &metadata.sender_wallet
        || acc.sender_tokens.key != &metadata.sender_tokens
        || acc.recipient_wallet.key != &metadata.recipient_wallet
        || acc.recipient_tokens.key != &metadata.recipient_tokens
        || acc.mint_account.key != &metadata.mint
        || acc.escrow_account.key != &metadata.escrow
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let now = Clock::get()?.unix_timestamp as u64;
    let available = metadata.available(now);

    let seeds = [acc.metadata_account.key.as_ref(), &[nonce]];

    invoke_signed(
        &spl_token::instruction::transfer(
            acc.token_program_account.key,
            acc.escrow_account.key,
            acc.recipient_tokens.key,
            acc.escrow_account.key,
            &[],
            available,
        )?,
        &[
            acc.escrow_account.clone(),
            acc.recipient_tokens.clone(),
            acc.escrow_account.clone(),
            acc.token_program_account.clone(),
        ],
        &[&seeds],
    )?;

    metadata.withdrawn += available;
    let remains = metadata.ix.amount - metadata.withdrawn;

    if remains > 0 {
        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program_account.key,
                acc.escrow_account.key,
                acc.sender_tokens.key,
                acc.escrow_account.key,
                &[],
                available,
            )?,
            &[
                acc.escrow_account.clone(),
                acc.sender_tokens.clone(),
                acc.escrow_account.clone(),
                acc.token_program_account.clone(),
            ],
            &[&seeds],
        )?;
    }

    // TODO: Check this for wrapped SOL
    let remains_escr = acc.escrow_account.lamports();
    let remains_meta = acc.metadata_account.lamports();

    **acc.escrow_account.try_borrow_mut_lamports()? -= remains_escr;
    **acc.sender_wallet.try_borrow_mut_lamports()? += remains_escr;
    **acc.metadata_account.try_borrow_mut_lamports()? -= remains_meta;
    **acc.sender_wallet.try_borrow_mut_lamports()? += remains_meta;

    msg!(
        "Transferred: {} {} tokens",
        encode_base10(available, mint_info.decimals.into()),
        metadata.mint
    );
    msg!(
        "Returned: {} {} tokens",
        encode_base10(remains, mint_info.decimals.into()),
        metadata.mint
    );
    msg!(
        "Returned rent: {} SOL",
        lamports_to_sol(remains_escr + remains_meta)
    );

    Ok(())
}

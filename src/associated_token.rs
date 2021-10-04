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
    account_info::{next_account_info, AccountInfo},
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

use crate::state::{TokenStreamData, TokenStreamInstruction};
use crate::utils::{
    duration_sanity, encode_base10, pretty_time, unpack_mint_account, unpack_token_account,
};

/// Initializes an SPL token stream
///
/// The account order:
/// * `sender_wallet` - The main wallet address of the initializer
/// * `sender_tokens` - The associated token account address of `sender_wallet`
/// * `recipient_wallet` - The main wallet address of the recipient
/// * `recipient_tokens` - The associated token account address of `recipient_wallet`
/// * `metadata` - The account holding the stream metadata
/// * `escrow` - The escrow account holding the stream funds
/// * `mint` - The SPL token mint account
/// * `rent` - The Rent sysvar account
/// * `timelock_program` - The program using this crate
/// * `token_program` - The SPL token program
/// * `associated_token_program` - The Associated Token program
/// * `system_program` - The Solana system program
///
/// The function shall initialize new accounts to hold the tokens,
/// and the stream's metadata. Both accounts will be funded to be
/// rent-exempt if necessary. When the stream is finished, these
/// shall be returned to the stream initializer.
pub fn initialize_token_stream(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    ix: TokenStreamInstruction,
) -> ProgramResult {
    msg!("Initializing SPL token stream");
    let account_info_iter = &mut accounts.iter();
    let sender_wallet = next_account_info(account_info_iter)?;
    let sender_tokens = next_account_info(account_info_iter)?;
    let recipient_wallet = next_account_info(account_info_iter)?;
    let recipient_tokens = next_account_info(account_info_iter)?;
    let metadata_account = next_account_info(account_info_iter)?;
    let escrow_account = next_account_info(account_info_iter)?;
    let mint_account = next_account_info(account_info_iter)?;
    let rent_account = next_account_info(account_info_iter)?;
    let timelock_program_account = next_account_info(account_info_iter)?;
    let token_program_account = next_account_info(account_info_iter)?;
    let _associated_token_program_account = next_account_info(account_info_iter)?;
    let system_program_account = next_account_info(account_info_iter)?;

    if !escrow_account.data_is_empty() || !metadata_account.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    if !sender_wallet.is_writable
        || !sender_tokens.is_writable
        || !recipient_wallet.is_writable
        || !recipient_tokens.is_writable
        || !metadata_account.is_writable
        || !escrow_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let (escrow_pubkey, nonce) =
        Pubkey::find_program_address(&[metadata_account.key.as_ref()], program_id);

    if system_program_account.key != &system_program::id()
        || token_program_account.key != &spl_token::id()
        || timelock_program_account.key != program_id
        || rent_account.key != &sysvar::rent::id()
        || escrow_account.key != &escrow_pubkey
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !sender_wallet.is_signer || !metadata_account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let sender_token_info = unpack_token_account(sender_tokens)?;
    let mint_info = unpack_mint_account(mint_account)?;

    if &sender_token_info.mint != mint_account.key {
        return Err(ProgramError::InvalidAccountData);
    }

    let now = Clock::get()?.unix_timestamp as u64;
    if !duration_sanity(now, ix.start_time, ix.end_time, ix.cliff) {
        msg!("Error: Given timestamps are invalid");
        return Err(ProgramError::InvalidArgument);
    }

    // We also transfer enough to be rent-exempt on the metadata account.
    // After all funds are unlocked and withdrawn, the remains are
    // returned to the sender's account.
    let metadata_struct_size = std::mem::size_of::<TokenStreamData>();
    let tokens_struct_size = spl_token::state::Account::LEN;
    let cluster_rent = Rent::get()?;
    let metadata_rent = cluster_rent.minimum_balance(metadata_struct_size);
    let mut tokens_rent = cluster_rent.minimum_balance(tokens_struct_size);
    let fees = Fees::get()?;
    let lps = fees.fee_calculator.lamports_per_signature;

    // Check if we have to initialize recipient's associated token account.
    if recipient_tokens.data_is_empty() {
        tokens_rent += cluster_rent.minimum_balance(tokens_struct_size);
        tokens_rent += lps;
    }

    if sender_wallet.lamports() < metadata_rent + tokens_rent + (4 * lps) {
        msg!("Error: Insufficient funds in {}", sender_wallet.key);
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
        *sender_wallet.key,
        *sender_tokens.key,
        *recipient_wallet.key,
        *recipient_tokens.key,
        *mint_account.key,
        *escrow_account.key,
        ix.period,
        ix.cliff,
        ix.cliff_amount,
    );
    let bytes = bincode::serialize(&metadata).unwrap();

    if recipient_tokens.data_is_empty() {
        msg!("Initializing recipient's associated token account");
        invoke(
            &create_associated_token_account(
                sender_wallet.key,
                recipient_wallet.key,
                mint_account.key,
            ),
            &[
                sender_wallet.clone(),
                recipient_tokens.clone(),
                recipient_wallet.clone(),
                mint_account.clone(),
                system_program_account.clone(),
                token_program_account.clone(),
                rent_account.clone(),
            ],
        )?;
    }

    msg!("Creating account for holding metadata");
    invoke(
        &system_instruction::create_account(
            sender_wallet.key,
            metadata_account.key,
            metadata_rent,
            metadata_struct_size as u64,
            program_id,
        ),
        &[
            sender_wallet.clone(),
            metadata_account.clone(),
            system_program_account.clone(),
        ],
    )?;

    // Write the metadata to the account
    let mut data = metadata_account.try_borrow_mut_data()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    let seeds = [metadata_account.key.as_ref(), &[nonce]];
    msg!("Creating account for holding tokens");
    invoke_signed(
        &system_instruction::create_account(
            sender_wallet.key,
            escrow_account.key,
            tokens_rent,
            tokens_struct_size as u64,
            &spl_token::id(),
        ),
        &[
            sender_wallet.clone(),
            escrow_account.clone(),
            system_program_account.clone(),
        ],
        &[&seeds],
    )?;

    msg!("Initializing escrow account for {} token", mint_account.key);
    invoke(
        &spl_token::instruction::initialize_account(
            token_program_account.key,
            escrow_account.key,
            mint_account.key,
            escrow_account.key,
        )?,
        &[
            token_program_account.clone(),
            escrow_account.clone(),
            mint_account.clone(),
            escrow_account.clone(),
            rent_account.clone(),
        ],
    )?;

    msg!("Moving funds into escrow account");
    invoke(
        &spl_token::instruction::transfer(
            token_program_account.key,
            sender_tokens.key,
            escrow_account.key,
            sender_wallet.key,
            &[],
            metadata.amount,
        )?,
        &[
            sender_tokens.clone(),
            escrow_account.clone(),
            sender_wallet.clone(),
            token_program_account.clone(),
        ],
    )?;

    msg!(
        "Successfully initialized {} {} token stream for {}",
        encode_base10(metadata.amount, mint_info.decimals.into()),
        metadata.mint,
        recipient_wallet.key
    );
    msg!("Called by {}", sender_wallet.key);
    msg!("Metadata written in {}", metadata_account.key);
    msg!("Funds locked in {}", escrow_account.key);
    msg!(
        "Stream duration is {}",
        pretty_time(metadata.end_time - metadata.start_time)
    );

    if metadata.cliff > 0 && metadata.cliff_amount > 0 {
        msg!("Cliff happens in {}", pretty_time(metadata.cliff));
    }

    Ok(())
}

/// Withdraws from an SPL Token stream
///
/// The account order:
/// * `sender_wallet` - The main wallet address of the initializer
/// * `sender_tokens` - The associated token account address of `sender_wallet`
/// * `recipient_wallet` - The main wallet address of the recipient
/// * `recipient_tokens` - The associated token account address of `recipient_wallet`
/// * `metadata` - The account holding the stream metadata
/// * `escrow` - The escrow account holding the stream funds
/// * `mint` - The SPL token mint account
/// * `rent` - The Rent sysvar account
/// * `timelock_program` - The program using this crate
/// * `token_program` - The SPL token program
/// * `system_program` - The Solana system program
///
/// The function will read the instructions from the metadata account and see
/// if there are any unlocked funds. If so, they will be transferred from the
/// escrow account to the stream recipient. If the entire amount has been
/// withdrawn, the remaining rents shall be returned to the stream initializer.
pub fn withdraw_token_stream(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
) -> ProgramResult {
    msg!("Withdrawing from SPL token stream");
    let account_info_iter = &mut accounts.iter();
    let sender_wallet = next_account_info(account_info_iter)?;
    let sender_tokens = next_account_info(account_info_iter)?;
    let recipient_wallet = next_account_info(account_info_iter)?;
    let recipient_tokens = next_account_info(account_info_iter)?;
    let metadata_account = next_account_info(account_info_iter)?;
    let escrow_account = next_account_info(account_info_iter)?;
    let mint_account = next_account_info(account_info_iter)?;
    let _rent_sysvar_account = next_account_info(account_info_iter)?;
    let _timelock_program_account = next_account_info(account_info_iter)?;
    let token_program_account = next_account_info(account_info_iter)?;
    let system_program_account = next_account_info(account_info_iter)?;

    if escrow_account.data_is_empty()
        || escrow_account.owner != &spl_token::id()
        || metadata_account.data_is_empty()
        || metadata_account.owner != program_id
    {
        return Err(ProgramError::UninitializedAccount);
    }

    if !sender_wallet.is_writable
        || !sender_tokens.is_writable
        || !recipient_wallet.is_writable
        || !recipient_tokens.is_writable
        || !metadata_account.is_writable
        || !escrow_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let (escrow_pubkey, nonce) =
        Pubkey::find_program_address(&[metadata_account.key.as_ref()], program_id);

    if system_program_account.key != &system_program::id()
        || token_program_account.key != &spl_token::id()
        || escrow_account.key != &escrow_pubkey
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !recipient_wallet.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut data = metadata_account.try_borrow_mut_data()?;
    let mut metadata = match bincode::deserialize::<TokenStreamData>(&data) {
        Ok(v) => v,
        Err(_) => return Err(ProgramError::Custom(143)),
    };

    let mint_info = unpack_mint_account(mint_account)?;

    if sender_wallet.key != &metadata.sender_wallet
        || sender_tokens.key != &metadata.sender_tokens
        || recipient_wallet.key != &metadata.recipient_wallet
        || recipient_tokens.key != &metadata.recipient_tokens
        || mint_account.key != &metadata.mint
        || escrow_account.key != &metadata.escrow
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

    let seeds = [metadata_account.key.as_ref(), &[nonce]];

    invoke_signed(
        &spl_token::instruction::transfer(
            token_program_account.key,
            escrow_account.key,
            recipient_tokens.key,
            escrow_account.key,
            &[],
            req,
        )?,
        &[
            escrow_account.clone(),
            recipient_tokens.clone(),
            escrow_account.clone(),
            token_program_account.clone(),
        ],
        &[&seeds],
    )?;

    metadata.withdrawn += req;

    let bytes = bincode::serialize(&metadata).unwrap();
    data[0..bytes.len()].clone_from_slice(&bytes);

    // Return rent when everything is withdrawn
    if metadata.withdrawn == metadata.amount {
        msg!("Returning rent to {}", sender_wallet.key);
        let rent = metadata_account.lamports();
        **metadata_account.try_borrow_mut_lamports()? -= rent;
        **sender_wallet.try_borrow_mut_lamports()? += rent;

        // TODO: Close token account, has to have close authority
    }

    msg!(
        "Withdrawn: {} {} tokens",
        encode_base10(req, mint_info.decimals.into()),
        metadata.mint
    );
    msg!(
        "Remaining: {} {} tokens",
        encode_base10(
            metadata.amount - metadata.withdrawn,
            mint_info.decimals.into()
        ),
        metadata.mint
    );

    Ok(())
}

/// Cancels an SPL Token stream
///
/// The account order:
/// * `sender_wallet` - The main wallet address of the initializer
/// * `sender_tokens` - The associated token account address of `sender_wallet`
/// * `recipient_wallet` - The main wallet address of the recipient
/// * `recipient_tokens` - The associated token account of `recipient_wallet`
/// * `metadata` - The account holding the stream metadata
/// * `escrow` - The escrow account holding the stream funds
/// * `mint` - The SPL token mint account
/// * `timelock_program` - The program using this crate
/// * `token_program` - The SPL token program
/// * `system_program` - The Solana system program
///
/// The function will read the instructions from the metadata account and see
/// if there are any unlocked funds. If so, they will be transferred to the
/// stream recipient, and any remains (including rents) shall be returned to
/// the stream initializer.
pub fn cancel_token_stream(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    msg!("Cancelling SPL token stream");
    let account_info_iter = &mut accounts.iter();
    let sender_wallet = next_account_info(account_info_iter)?;
    let sender_tokens = next_account_info(account_info_iter)?;
    let recipient_wallet = next_account_info(account_info_iter)?;
    let recipient_tokens = next_account_info(account_info_iter)?;
    let metadata_account = next_account_info(account_info_iter)?;
    let escrow_account = next_account_info(account_info_iter)?;
    let mint_account = next_account_info(account_info_iter)?;
    let _timelock_program_account = next_account_info(account_info_iter)?;
    let token_program_account = next_account_info(account_info_iter)?;
    let system_program_account = next_account_info(account_info_iter)?;

    if escrow_account.data_is_empty()
        || escrow_account.owner != &spl_token::id()
        || metadata_account.data_is_empty()
        || metadata_account.owner != program_id
    {
        return Err(ProgramError::UninitializedAccount);
    }

    if !sender_wallet.is_writable
        || !sender_tokens.is_writable
        || !recipient_wallet.is_writable
        || !recipient_tokens.is_writable
        || !metadata_account.is_writable
        || !escrow_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let (escrow_pubkey, nonce) =
        Pubkey::find_program_address(&[metadata_account.key.as_ref()], program_id);

    if system_program_account.key != &system_program::id()
        || token_program_account.key != &spl_token::id()
        || escrow_account.key != &escrow_pubkey
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !sender_wallet.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let data = metadata_account.try_borrow_mut_data()?;
    let mut metadata = match bincode::deserialize::<TokenStreamData>(&data) {
        Ok(v) => v,
        Err(_) => return Err(ProgramError::Custom(143)),
    };

    let mint_info = unpack_mint_account(mint_account)?;

    if sender_wallet.key != &metadata.sender_wallet
        || sender_tokens.key != &metadata.sender_tokens
        || recipient_wallet.key != &metadata.recipient_wallet
        || recipient_tokens.key != &metadata.recipient_tokens
        || mint_account.key != &metadata.mint
        || escrow_account.key != &metadata.escrow
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let now = Clock::get()?.unix_timestamp as u64;
    let available = metadata.available(now);

    let seeds = [metadata_account.key.as_ref(), &[nonce]];

    invoke_signed(
        &spl_token::instruction::transfer(
            token_program_account.key,
            escrow_account.key,
            recipient_tokens.key,
            escrow_account.key,
            &[],
            available,
        )?,
        &[
            escrow_account.clone(),
            recipient_tokens.clone(),
            escrow_account.clone(),
            token_program_account.clone(),
        ],
        &[&seeds],
    )?;

    metadata.withdrawn += available;
    let remains = metadata.amount - metadata.withdrawn;

    if remains > 0 {
        invoke_signed(
            &spl_token::instruction::transfer(
                token_program_account.key,
                escrow_account.key,
                sender_tokens.key,
                escrow_account.key,
                &[],
                available,
            )?,
            &[
                escrow_account.clone(),
                sender_tokens.clone(),
                escrow_account.clone(),
                token_program_account.clone(),
            ],
            &[&seeds],
        )?;
    }

    let remains_escr = escrow_account.lamports();
    let remains_meta = metadata_account.lamports();

    **escrow_account.try_borrow_mut_lamports()? -= remains_escr;
    **sender_wallet.try_borrow_mut_lamports()? += remains_escr;
    **metadata_account.try_borrow_mut_lamports()? -= remains_meta;
    **sender_wallet.try_borrow_mut_lamports()? += remains_meta;

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

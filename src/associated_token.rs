// Copyright (c) 2021 Ivan Jelincic <parazyd@dyne.org>
//
// This file is part of streamflow-finance/timelock-crate
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
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    entrypoint::ProgramResult,
    msg,
    // native_token::lamports_to_sol,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    system_instruction,
    system_program,
    sysvar,
    sysvar::{clock::Clock, fees::Fees, rent::Rent, Sysvar},
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};

use crate::state::{
    CancelAccounts, InitializeAccounts, StreamInstruction, TokenStreamData, TransferAccounts,
    WithdrawAccounts,
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

    if !acc.escrow_tokens.data_is_empty() || !acc.metadata.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    if !acc.sender.is_writable
        || !acc.sender_tokens.is_writable
        || !acc.recipient.is_writable // TODO: Could it be read-only?
        || !acc.recipient_tokens.is_writable
        || !acc.metadata.is_writable
        || !acc.escrow_tokens.is_writable
    {
        return Err(ProgramError::Custom(1)); //TODO: Add custom error "Account not writeable"
    }

    let (escrow_tokens_pubkey, nonce) =
        Pubkey::find_program_address(&[acc.metadata.key.as_ref()], program_id);
    let recipient_tokens_key = get_associated_token_address(acc.recipient.key, acc.mint.key);

    if acc.system_program.key != &system_program::id()
        || acc.token_program.key != &spl_token::id()
        || acc.rent.key != &sysvar::rent::id()
        || acc.escrow_tokens.key != &escrow_tokens_pubkey
        || acc.recipient_tokens.key != &recipient_tokens_key
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !acc.sender.is_signer || !acc.metadata.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let sender_token_info = unpack_token_account(&acc.sender_tokens)?;
    let mint_info = unpack_mint_account(&acc.mint)?;

    if &sender_token_info.mint != acc.mint.key {
        return Err(ProgramError::Custom(3)); //mint missmatch
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
    let escrow_tokens_rent = cluster_rent.minimum_balance(tokens_struct_size);
    let recipient_tokens_rent = if acc.recipient_tokens.data_is_empty() {
        cluster_rent.minimum_balance(tokens_struct_size)
    } else {
        0
    };
    let fees = Fees::get()?;
    let lps = fees.fee_calculator.lamports_per_signature;

    // TODO: Check if wrapped SOL
    if acc.sender.lamports()
        < metadata_rent + escrow_tokens_rent + recipient_tokens_rent + (2 * lps)
    {
        msg!("Error: Insufficient funds in {}", acc.sender.key);
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
        *acc.sender.key,
        *acc.sender_tokens.key,
        *acc.recipient.key,
        *acc.recipient_tokens.key,
        *acc.mint.key,
        *acc.escrow_tokens.key,
    );
    //let bytes = bincode::serialize(&metadata).unwrap();
    let bytes = metadata.try_to_vec().unwrap();

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

    msg!("Creating account for holding metadata");
    invoke(
        &system_instruction::create_account(
            acc.sender.key,
            acc.metadata.key,
            metadata_rent,
            metadata_struct_size as u64,
            program_id,
        ),
        &[
            acc.sender.clone(),
            acc.metadata.clone(),
            acc.system_program.clone(),
        ],
    )?;

    // Write the metadata to the account
    let mut data = acc.metadata.try_borrow_mut_data()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    let seeds = [acc.metadata.key.as_ref(), &[nonce]];
    msg!("Creating account for holding tokens");
    invoke_signed(
        &system_instruction::create_account(
            acc.sender.key,
            acc.escrow_tokens.key,
            escrow_tokens_rent,
            tokens_struct_size as u64,
            &spl_token::id(),
        ),
        &[
            acc.sender.clone(),
            acc.escrow_tokens.clone(),
            acc.system_program.clone(),
        ],
        &[&seeds],
    )?;

    msg!("Initializing escrow account for {} token", acc.mint.key);
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
            acc.escrow_tokens.clone(),
            acc.rent.clone(),
        ],
    )?;

    msg!("Moving funds into escrow account");
    invoke(
        &spl_token::instruction::transfer(
            acc.token_program.key,
            acc.sender_tokens.key,
            acc.escrow_tokens.key,
            acc.sender.key,
            &[],
            metadata.ix.amount,
        )?,
        &[
            acc.sender_tokens.clone(),
            acc.escrow_tokens.clone(),
            acc.sender.clone(),
            acc.token_program.clone(),
        ],
    )?;

    msg!(
        "Successfully initialized {} {} token stream for {}",
        encode_base10(metadata.ix.amount, mint_info.decimals.into()),
        metadata.mint,
        acc.recipient.key
    );
    msg!("Called by {}", acc.sender.key);
    msg!("Metadata written in {}", acc.metadata.key);
    msg!("Funds locked in {}", acc.escrow_tokens.key);
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

    if acc.escrow_tokens.data_is_empty()
        || acc.escrow_tokens.owner != &spl_token::id()
        || acc.metadata.data_is_empty()
        || acc.metadata.owner != program_id
    {
        return Err(ProgramError::UninitializedAccount);
    }

    if !acc.recipient.is_writable
        || !acc.recipient_tokens.is_writable
        || !acc.metadata.is_writable
        || !acc.escrow_tokens.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let (escrow_tokens_pubkey, nonce) =
        Pubkey::find_program_address(&[acc.metadata.key.as_ref()], program_id);
    let recipient_tokens_key = get_associated_token_address(acc.recipient.key, acc.mint.key);

    if acc.token_program.key != &spl_token::id()
        || acc.escrow_tokens.key != &escrow_tokens_pubkey
        || acc.recipient_tokens.key != &recipient_tokens_key
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !acc.recipient.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut data = acc.metadata.try_borrow_mut_data()?;
    //let mut metadata = match bincode::deserialize::<TokenStreamData>(&data) {
    let mut metadata = match TokenStreamData::try_from_slice(&data) {
        Ok(v) => v,
        Err(_) => return Err(ProgramError::Custom(1)), // TODO: Add "Invalid Metadata" as an error
    };

    let mint_info = unpack_mint_account(&acc.mint)?;

    if acc.recipient.key != &metadata.recipient
        || acc.recipient_tokens.key != &metadata.recipient_tokens
        || acc.mint.key != &metadata.mint
        || acc.escrow_tokens.key != &metadata.escrow_tokens
    {
        msg!("Error: Metadata does not match given accounts");
        return Err(ProgramError::InvalidAccountData);
    }

    let now = Clock::get()?.unix_timestamp as u64;
    let available = metadata.available(now);
    let requested: u64;

    if amount > available {
        msg!("Amount requested for withdraw is more than what is available");
        return Err(ProgramError::InvalidArgument);
    }

    // 0 == MAX
    if amount == 0 {
        requested = available;
    } else {
        requested = amount;
    }

    let seeds = [acc.metadata.key.as_ref(), &[nonce]];
    invoke_signed(
        &spl_token::instruction::transfer(
            acc.token_program.key,
            acc.escrow_tokens.key,
            acc.recipient_tokens.key,
            acc.escrow_tokens.key,
            &[],
            requested,
        )?,
        &[
            acc.escrow_tokens.clone(),    // src
            acc.recipient_tokens.clone(), // dest
            acc.escrow_tokens.clone(),    // auth
            acc.token_program.clone(),    // program
        ],
        &[&seeds],
    )?;

    metadata.withdrawn += requested;
    //let bytes = bincode::serialize(&metadata).unwrap();
    let bytes = metadata.try_to_vec().unwrap();
    data[0..bytes.len()].clone_from_slice(&bytes);

    // Return rent when everything is withdrawn
    // if metadata.withdrawn == metadata.ix.amount {
    //     msg!("Returning rent to {}", acc.sender.key);
    //     let rent = acc.metadata.lamports();
    //     **acc.metadata.try_borrow_mut_lamports()? -= rent;
    //     **acc.sender.try_borrow_mut_lamports()? += rent;
    //
    //     // TODO: Close token account, has to have close authority
    // }

    msg!(
        "Withdrawn: {} {} tokens",
        encode_base10(requested, mint_info.decimals.into()),
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
/// stream recipient.
pub fn cancel_token_stream(program_id: &Pubkey, acc: CancelAccounts) -> ProgramResult {
    msg!("Cancelling SPL token stream");

    if acc.escrow_tokens.data_is_empty()
        || acc.escrow_tokens.owner != &spl_token::id()
        || acc.metadata.data_is_empty()
        || acc.metadata.owner != program_id
    {
        return Err(ProgramError::UninitializedAccount);
    }

    if !acc.sender.is_writable
        || !acc.sender_tokens.is_writable
        || !acc.recipient.is_writable // TODO: Might not be needed
        || !acc.recipient_tokens.is_writable
        || !acc.metadata.is_writable
        || !acc.escrow_tokens.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let (escrow_tokens_pubkey, nonce) =
        Pubkey::find_program_address(&[acc.metadata.key.as_ref()], program_id);
    let recipient_tokens_key = get_associated_token_address(acc.recipient.key, acc.mint.key);

    if acc.token_program.key != &spl_token::id()
        || acc.escrow_tokens.key != &escrow_tokens_pubkey
        || acc.recipient_tokens.key != &recipient_tokens_key
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !acc.sender.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let data = acc.metadata.try_borrow_mut_data()?;
    //let mut metadata = match bincode::deserialize::<TokenStreamData>(&data) {
    let mut metadata = match TokenStreamData::try_from_slice(&data) {
        Ok(v) => v,
        Err(_) => return Err(ProgramError::Custom(143)),
    };

    let mint_info = unpack_mint_account(&acc.mint)?;

    if acc.sender.key != &metadata.sender
        || acc.sender_tokens.key != &metadata.sender_tokens
        || acc.recipient.key != &metadata.recipient
        || acc.recipient_tokens.key != &metadata.recipient_tokens
        || acc.mint.key != &metadata.mint
        || acc.escrow_tokens.key != &metadata.escrow_tokens
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let now = Clock::get()?.unix_timestamp as u64;
    let available = metadata.available(now);

    let seeds = [acc.metadata.key.as_ref(), &[nonce]];
    invoke_signed(
        &spl_token::instruction::transfer(
            acc.token_program.key,
            acc.escrow_tokens.key,
            acc.recipient_tokens.key,
            acc.escrow_tokens.key,
            &[],
            available,
        )?,
        &[
            acc.escrow_tokens.clone(),    // src
            acc.recipient_tokens.clone(), // dest
            acc.escrow_tokens.clone(),    // auth
            acc.token_program.clone(),    // program
        ],
        &[&seeds],
    )?;

    metadata.withdrawn += available;
    let remains = metadata.ix.amount - metadata.withdrawn;

    // Return any remaining funds to the stream initializer
    if remains > 0 {
        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.sender_tokens.key,
                acc.escrow_tokens.key,
                &[],
                remains,
            )?,
            &[
                acc.escrow_tokens.clone(),
                acc.sender_tokens.clone(),
                acc.escrow_tokens.clone(),
                acc.token_program.clone(),
            ],
            &[&seeds],
        )?;
    }

    // TODO: Check this for wrapped SOL
    // let remains_escrow_tokens = acc.escrow_tokens.lamports();
    // let remains_meta = acc.metadata.lamports();
    //
    // **acc.escrow_tokens.try_borrow_mut_lamports()? -= remains_escrow_tokens;
    // **acc.sender.try_borrow_mut_lamports()? += remains_escrow_tokens;
    // **acc.metadata.try_borrow_mut_lamports()? -= remains_meta;
    // **acc.sender.try_borrow_mut_lamports()? += remains_meta;

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
    // msg!(
    //     "Returned rent: {} SOL",
    //     lamports_to_sol(remains_escrow_tokens + remains_meta)
    // );

    Ok(())
}

pub fn update_recipient(program_id: &Pubkey, acc: TransferAccounts) -> ProgramResult {
    msg!("Transferring stream recipient");
    if acc.metadata.data_is_empty()
        || acc.metadata.owner != program_id
        || acc.escrow_tokens.data_is_empty()
        || acc.escrow_tokens.owner != &spl_token::id()
    {
        return Err(ProgramError::UninitializedAccount);
    }

    if !acc.existing_recipient.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    if !acc.metadata.is_writable
        || !acc.existing_recipient.is_writable
        || !acc.new_recipient_tokens.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let mut data = acc.metadata.try_borrow_mut_data()?;
    //let mut metadata = match bincode::deserialize::<TokenStreamData>(&data) {
    let mut metadata = match TokenStreamData::try_from_slice(&data) {
        Ok(v) => v,
        Err(_) => return Err(ProgramError::Custom(1)), // TODO: Add "Invalid Metadata" as an error
    };

    let (escrow_tokens_pubkey, _) =
        Pubkey::find_program_address(&[acc.metadata.key.as_ref()], program_id);
    let new_recipient_tokens_key =
        get_associated_token_address(acc.new_recipient.key, acc.mint.key);

    if acc.new_recipient_tokens.key != &new_recipient_tokens_key
        || acc.mint.key != &metadata.mint
        || acc.existing_recipient.key != &metadata.recipient
        || acc.escrow_tokens.key != &metadata.escrow_tokens
        || acc.escrow_tokens.key != &escrow_tokens_pubkey
        || acc.token_program.key != &spl_token::id()
        || acc.system_program.key != &system_program::id()
        || acc.rent.key != &sysvar::rent::id()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if acc.new_recipient_tokens.data_is_empty() {
        // Initialize a new_beneficiary_owner account
        let tokens_struct_size = spl_token::state::Account::LEN;
        let cluster_rent = Rent::get()?;
        let tokens_rent = cluster_rent.minimum_balance(tokens_struct_size);
        let fees = Fees::get()?;
        let lps = fees.fee_calculator.lamports_per_signature;

        // TODO: Check if wrapped SOL
        if acc.existing_recipient.lamports() < tokens_rent + lps {
            msg!(
                "Error: Insufficient funds in {}",
                acc.existing_recipient.key
            );
            return Err(ProgramError::InsufficientFunds);
        }

        msg!("Initializing new recipient's associated token account");
        invoke(
            &create_associated_token_account(
                acc.existing_recipient.key,
                acc.new_recipient.key,
                acc.mint.key,
            ),
            &[
                acc.existing_recipient.clone(),   // Funding
                acc.new_recipient_tokens.clone(), // Associated token account's address
                acc.new_recipient.clone(),        // Wallet address
                acc.mint.clone(),
                acc.system_program.clone(),
                acc.token_program.clone(),
                acc.rent.clone(),
            ],
        )?;
    }

    // Update recipient
    metadata.recipient = *acc.new_recipient.key;
    metadata.recipient_tokens = *acc.new_recipient_tokens.key;

    //let bytes = bincode::serialize(&metadata).unwrap();
    let bytes = metadata.try_to_vec().unwrap();
    data[0..bytes.len()].clone_from_slice(&bytes);

    Ok(())
}

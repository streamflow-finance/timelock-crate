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
    program::invoke,
    program_error::ProgramError,
    pubkey::Pubkey,
    system_instruction,
    sysvar::{clock::Clock, rent::Rent, Sysvar},
};

use crate::state::{NativeStreamData, TokenStreamInstruction};
use crate::utils::{duration_sanity, pretty_time};

/// Initializes a native SOL stream
///
/// The account order:
/// * `sender` - The initializer of the stream
/// * `recipient` - The recipient of the stream
/// * `escrow` - The escrow account of the stream
/// * `system_program` - The Solana system program
///
/// The function shall initialize a new escrow account and deposit
/// funds and fill it with the stream's metadata. Along with the
/// requested streaming amount, additional funds will be deposited
/// so the account becomes rent-exempt. When the stream is finished,
/// these shall be returned to the stream initializer.
pub fn initialize_native_stream(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    ix: TokenStreamInstruction,
) -> ProgramResult {
    msg!("Initializing native SOL stream");
    let account_info_iter = &mut accounts.iter();
    let sender_account = next_account_info(account_info_iter)?;
    let recipient_account = next_account_info(account_info_iter)?;
    let escrow_account = next_account_info(account_info_iter)?;
    let system_program_account = next_account_info(account_info_iter)?;

    if !escrow_account.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    if !sender_account.is_writable || !recipient_account.is_writable || !escrow_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if system_program_account.key != &solana_program::system_program::id() {
        return Err(ProgramError::InvalidAccountData);
    }

    if !sender_account.is_signer || !escrow_account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // We also transfer enough to be rent-exempt to the new account.
    // After all funds are withdrawn and unlocked, the remains are
    // returned to the sender's account.
    let struct_size = std::mem::size_of::<NativeStreamData>();
    let cluster_rent = Rent::get()?;

    if sender_account.lamports() < ix.amount + cluster_rent.minimum_balance(struct_size) {
        return Err(ProgramError::InsufficientFunds);
    }

    let now = Clock::get()?.unix_timestamp as u64;
    if !duration_sanity(now, ix.start_time, ix.end_time) {
        return Err(ProgramError::InvalidArgument);
    }

    let metadata = NativeStreamData::new(
        ix.start_time,
        ix.end_time,
        ix.amount,
        *sender_account.key,
        *recipient_account.key,
        *escrow_account.key,
        ix.period,
        ix.cliff,
        ix.cliff_amount,
    );
    let bytes = bincode::serialize(&metadata).unwrap();

    // Create the escrow account holding locked funds and metadata.
    // The program_id passed in as the function's argument is the
    // account owner. This gives it control over the withdrawal
    // process.
    msg!("Creating escrow account for holding funds and metadata");
    invoke(
        &system_instruction::create_account(
            sender_account.key,
            escrow_account.key,
            metadata.amount + cluster_rent.minimum_balance(struct_size),
            struct_size as u64,
            program_id,
        ),
        &[
            sender_account.clone(),
            escrow_account.clone(),
            system_program_account.clone(),
        ],
    )?;

    // Write the metadata to the escrow account
    let mut data = escrow_account.try_borrow_mut_data()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    msg!(
        "Successfully initialized {} SOL stream for {}",
        lamports_to_sol(metadata.amount),
        recipient_account.key
    );
    msg!("Called by {}", sender_account.key);
    msg!("Funds locked in {}", escrow_account.key);
    msg!(
        "Stream duration is {}",
        pretty_time(metadata.end_time - metadata.start_time)
    );

    Ok(())
}

/// Withdraws from a native SOL stream
///
/// The account order:
/// * `sender` - The stream initializer
/// * `recipient` - The stream recipient
/// * `escrow` - The stream escrow account
///
/// The function will read the escrow account's metadata and see if there are
/// any unlocked funds. If so, they will be transferred to the stream recipient,
/// If the entire amount has been withdrawn, the remaining rent will be returned
/// to the stream initializer.
pub fn withdraw_native_stream(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
) -> ProgramResult {
    msg!("Withdrawing from SOL stream");
    let account_info_iter = &mut accounts.iter();
    let sender_account = next_account_info(account_info_iter)?;
    let recipient_account = next_account_info(account_info_iter)?;
    let escrow_account = next_account_info(account_info_iter)?;

    if escrow_account.data_is_empty() || escrow_account.owner != program_id {
        return Err(ProgramError::UninitializedAccount);
    }

    if !sender_account.is_writable || !recipient_account.is_writable || !escrow_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !recipient_account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut data = escrow_account.try_borrow_mut_data()?;
    let mut metadata = match bincode::deserialize::<NativeStreamData>(&data) {
        Ok(v) => v,
        Err(_) => return Err(ProgramError::Custom(143)),
    };

    if sender_account.key != &metadata.sender || recipient_account.key != &metadata.recipient {
        return Err(ProgramError::InvalidAccountData);
    }

    let now = Clock::get()?.unix_timestamp as u64;
    let available = metadata.available(now);

    if amount > available {
        msg!("Amount requested for withdraw is more than what is available");
        return Err(ProgramError::InvalidArgument);
    }

    **escrow_account.try_borrow_mut_lamports()? -= amount;
    **recipient_account.try_borrow_mut_lamports()? += amount;
    metadata.withdrawn += available;

    let bytes = bincode::serialize(&metadata).unwrap();
    data[0..bytes.len()].clone_from_slice(&bytes);

    // Return rent when everything is withdrawn
    if metadata.withdrawn == metadata.amount {
        msg!("Returning rent to {}", sender_account.key);
        let rent = escrow_account.lamports();
        **escrow_account.try_borrow_mut_lamports()? -= rent;
        **sender_account.try_borrow_mut_lamports()? += rent;
    }

    msg!("Withdrawn: {} SOL", lamports_to_sol(available));
    msg!(
        "Remaining: {} SOL",
        lamports_to_sol(metadata.amount - metadata.withdrawn)
    );

    Ok(())
}

/// Cancels a native SOL stream
///
/// The account order:
/// * `sender` - The initializer of the stream
/// * `recipient` - The recipient of the stream
/// * `escrow` - The escrow account of the stream
///
/// The function will read the escrow account's metadata and see if there are
/// any unlocked funds. If so, they will be transferred to the stream recipient,
/// and any remains (including rent) will be returned to the stream initializer.
pub fn cancel_native_stream(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    msg!("Cancelling native SOL stream");
    let account_info_iter = &mut accounts.iter();
    let sender_account = next_account_info(account_info_iter)?;
    let recipient_account = next_account_info(account_info_iter)?;
    let escrow_account = next_account_info(account_info_iter)?;

    if escrow_account.data_is_empty() || escrow_account.owner != program_id {
        return Err(ProgramError::UninitializedAccount);
    }

    if !sender_account.is_writable || !recipient_account.is_writable || !escrow_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !sender_account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let data = escrow_account.try_borrow_data()?;
    let metadata: NativeStreamData;
    match bincode::deserialize::<NativeStreamData>(&data) {
        Ok(v) => metadata = v,
        Err(_) => return Err(ProgramError::Custom(143)),
    };

    if sender_account.key != &metadata.sender {
        return Err(ProgramError::Custom(144));
    }

    if recipient_account.key != &metadata.recipient {
        return Err(ProgramError::Custom(144));
    }

    let now = Clock::get()?.unix_timestamp as u64;
    let available = metadata.available(now);

    // Transfer what was unlocked but not withdrawn to the recipient.
    **escrow_account.try_borrow_mut_lamports()? -= available;
    **recipient_account.try_borrow_mut_lamports()? += available;

    // And return the rest to the stream initializer.
    let remains = escrow_account.lamports();
    **escrow_account.try_borrow_mut_lamports()? -= remains;
    **sender_account.try_borrow_mut_lamports()? += remains;

    msg!("Transferred: {} SOL", lamports_to_sol(available));
    msg!("Returned: {} SOL", lamports_to_sol(remains));

    Ok(())
}

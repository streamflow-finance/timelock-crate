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
    program::invoke,
    program_error::ProgramError,
    pubkey::Pubkey,
    system_instruction,
    sysvar::{clock::Clock, fees::Fees, rent::Rent, Sysvar},
};

use crate::state::{
    NativeStreamCancelAccounts, NativeStreamData, NativeStreamInitAccounts,
    NativeStreamWithdrawAccounts, StreamInstruction,
};
use crate::utils::{duration_sanity, pretty_time};

/// Initialize a native SOL stream
///
/// The function shall initialize a new escrow account and deposit
/// funds and fill it with the stream's metadata. Along with the
/// requested streaming amount, additional funds will be deposited
/// so the account becomes rent-exempt. When the stream is finished,
/// these shall be returned to the stream initializer.
pub fn initialize_native_stream(
    program_id: &Pubkey,
    acc: NativeStreamInitAccounts,
    ix: StreamInstruction,
) -> ProgramResult {
    msg!("Initializing native SOL stream");

    if !acc.escrow_account.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    if !acc.sender_wallet.is_writable
        || !acc.recipient_wallet.is_writable
        || !acc.escrow_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if acc.system_program_account.key != &solana_program::system_program::id() {
        return Err(ProgramError::InvalidAccountData);
    }

    if !acc.sender_wallet.is_signer || !acc.escrow_account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let now = Clock::get()?.unix_timestamp as u64;
    if !duration_sanity(now, ix.start_time, ix.end_time, ix.cliff) {
        msg!("Error: Given timestamps are invalid");
        return Err(ProgramError::InvalidArgument);
    }

    // We also transfer enough to be rent-exempt to the new account.
    // After all funds are withdrawn and unlocked, the remains are
    // returned to the sender's account.
    let struct_size = std::mem::size_of::<NativeStreamData>();
    let cluster_rent = Rent::get()?;
    let fees = Fees::get()?;
    let lps = fees.fee_calculator.lamports_per_signature;

    if acc.sender_wallet.lamports()
        < ix.amount + cluster_rent.minimum_balance(struct_size) + (2 * lps)
    {
        msg!("Error: Insufficient funds in {}", acc.sender_wallet.key);
        return Err(ProgramError::InsufficientFunds);
    }

    let metadata = NativeStreamData::new(
        ix.start_time,
        ix.end_time,
        ix.amount,
        ix.period,
        ix.cliff,
        ix.cliff_amount,
        *acc.sender_wallet.key,
        *acc.recipient_wallet.key,
        *acc.escrow_account.key,
    );
    let bytes = bincode::serialize(&metadata).unwrap();

    // Create the escrow account holding locked funds and metadata.
    // The program_id passed in as the function's argument is the
    // account owner. This gives it control over the withdrawal
    // process.
    msg!("Creating account for holding funds and metadata");
    invoke(
        &system_instruction::create_account(
            acc.sender_wallet.key,
            acc.escrow_account.key,
            metadata.ix.amount + cluster_rent.minimum_balance(struct_size),
            struct_size as u64,
            program_id,
        ),
        &[
            acc.sender_wallet.clone(),
            acc.escrow_account.clone(),
            acc.system_program_account.clone(),
        ],
    )?;

    // Write the metadata to the escrow account
    let mut data = acc.escrow_account.try_borrow_mut_data()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    msg!(
        "Successfully initialized {} SOL stream for {}",
        lamports_to_sol(metadata.ix.amount),
        acc.recipient_wallet.key
    );
    msg!("Called by {}", acc.sender_wallet.key);
    msg!("Funds locked in {}", acc.escrow_account.key);
    msg!(
        "Stream duration is {}",
        pretty_time(metadata.ix.end_time - metadata.ix.start_time)
    );

    if metadata.ix.cliff > 0 && metadata.ix.cliff_amount > 0 {
        msg!("Cliff happens in {}", pretty_time(metadata.ix.cliff));
    }

    Ok(())
}

/// Withdraw from a native SOL stream
///
/// The function will read the escrow account's metadata and see if there are
/// any unlocked funds. If so, they will be transferred to the stream recipient,
/// If the entire amount has been withdrawn, the remaining rent will be returned
/// to the stream initializer.
pub fn withdraw_native_stream(
    program_id: &Pubkey,
    acc: NativeStreamWithdrawAccounts,
    amount: u64,
) -> ProgramResult {
    msg!("Withdrawing from SOL stream");

    if acc.escrow_account.data_is_empty() || acc.escrow_account.owner != program_id {
        return Err(ProgramError::UninitializedAccount);
    }

    if !acc.sender_wallet.is_writable
        || !acc.recipient_wallet.is_writable
        || !acc.escrow_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !acc.recipient_wallet.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut data = acc.escrow_account.try_borrow_mut_data()?;
    let mut metadata = match bincode::deserialize::<NativeStreamData>(&data) {
        Ok(v) => v,
        Err(_) => return Err(ProgramError::Custom(143)),
    };

    if acc.sender_wallet.key != &metadata.sender || acc.recipient_wallet.key != &metadata.recipient
    {
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

    **acc.escrow_account.try_borrow_mut_lamports()? -= req;
    **acc.recipient_wallet.try_borrow_mut_lamports()? += req;
    metadata.withdrawn += req;

    let bytes = bincode::serialize(&metadata).unwrap();
    data[0..bytes.len()].clone_from_slice(&bytes);

    // Return rent when everything is withdrawn
    if metadata.withdrawn == metadata.ix.amount {
        msg!("Returning rent to {}", acc.sender_wallet.key);
        let rent = acc.escrow_account.lamports();
        **acc.escrow_account.try_borrow_mut_lamports()? -= rent;
        **acc.sender_wallet.try_borrow_mut_lamports()? += rent;
    }

    msg!("Withdrawn: {} SOL", lamports_to_sol(req));
    msg!(
        "Remaining: {} SOL",
        lamports_to_sol(metadata.ix.amount - metadata.withdrawn)
    );

    Ok(())
}

/// Cancel a native SOL stream
///
/// The function will read the escrow account's metadata and see if there are
/// any unlocked funds. If so, they will be transferred to the stream recipient,
/// and any remains (including rent) will be returned to the stream initializer.
pub fn cancel_native_stream(program_id: &Pubkey, acc: NativeStreamCancelAccounts) -> ProgramResult {
    msg!("Cancelling native SOL stream");

    if acc.escrow_account.data_is_empty() || acc.escrow_account.owner != program_id {
        return Err(ProgramError::UninitializedAccount);
    }

    if !acc.sender_wallet.is_writable
        || !acc.recipient_wallet.is_writable
        || !acc.escrow_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !acc.sender_wallet.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let data = acc.escrow_account.try_borrow_data()?;
    let metadata = match bincode::deserialize::<NativeStreamData>(&data) {
        Ok(v) => v,
        Err(_) => return Err(ProgramError::Custom(143)),
    };

    if acc.sender_wallet.key != &metadata.sender {
        return Err(ProgramError::Custom(144));
    }

    if acc.recipient_wallet.key != &metadata.recipient {
        return Err(ProgramError::Custom(144));
    }

    let now = Clock::get()?.unix_timestamp as u64;
    let available = metadata.available(now);

    // Transfer what was unlocked but not withdrawn to the recipient.
    **acc.escrow_account.try_borrow_mut_lamports()? -= available;
    **acc.recipient_wallet.try_borrow_mut_lamports()? += available;

    // And return the rest to the stream initializer.
    let remains = acc.escrow_account.lamports();
    **acc.escrow_account.try_borrow_mut_lamports()? -= remains;
    **acc.sender_wallet.try_borrow_mut_lamports()? += remains;

    msg!("Transferred: {} SOL", lamports_to_sol(available));
    msg!("Returned: {} SOL", lamports_to_sol(remains));

    Ok(())
}

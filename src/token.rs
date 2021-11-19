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
    borsh as solana_borsh,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    system_instruction, system_program, sysvar,
    sysvar::{clock::Clock, fees::Fees, rent::Rent, Sysvar},
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};

use crate::state::{
    CancelAccounts, InitializeAccounts, StreamInstruction, TokenStreamData, TopUpAccounts,
    TransferAccounts, WithdrawAccounts,
};
use crate::utils::{
    duration_sanity, encode_base10, pretty_time, unpack_mint_account, unpack_token_account,
};

use crate::error::StreamFlowError::{AccountsNotWritable, InvalidMetaData, MintMismatch};
/// Initialize an SPL token stream
///
/// The function shall initialize new accounts to hold the tokens,
/// and the stream's metadata. Both accounts will be funded to be
/// rent-exempt if necessary. When the stream is finished, these
/// shall be returned to the stream initializer.
pub fn create(
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
        return Err(AccountsNotWritable.into());
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
        // Mint mismatch
        return Err(MintMismatch.into());
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
    if acc.recipient_tokens.data_is_empty() {
        tokens_rent += cluster_rent.minimum_balance(tokens_struct_size);
    }

    let fees = Fees::get()?;
    let lps = fees.fee_calculator.lamports_per_signature;

    // TODO: Check if wrapped SOL
    if acc.sender.lamports() < metadata_rent + tokens_rent + (2 * lps) {
        msg!("Error: Insufficient funds in {}", acc.sender.key);
        return Err(ProgramError::InsufficientFunds);
    }

    if sender_token_info.amount < ix.total_amount {
        msg!("Error: Insufficient tokens in sender's wallet");
        return Err(ProgramError::InsufficientFunds);
    }

    // TODO: Calculate cancel_data once continuous streams are ready
    let mut metadata = TokenStreamData::new(
        now,
        *acc.sender.key,
        *acc.sender_tokens.key,
        *acc.recipient.key,
        *acc.recipient_tokens.key,
        *acc.mint.key,
        *acc.escrow_tokens.key,
        ix.start_time,
        ix.end_time,
        ix.deposited_amount,
        ix.total_amount,
        ix.release_rate,
        ix.period,
        ix.cliff,
        ix.cliff_amount,
        ix.cancelable_by_sender,
        ix.cancelable_by_recipient,
        ix.withdrawal_public,
        ix.transferable,
        ix.stream_name,
    );

    // Move closable_at (from third party), when reccuring ignore end_date
    if ix.deposited_amount < ix.total_amount || ix.release_rate > 0 {
        metadata.closable_at = metadata.closable();
    }

    let bytes = metadata.try_to_vec()?;

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
            cluster_rent.minimum_balance(tokens_struct_size),
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
            metadata.ix.deposited_amount,
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
        encode_base10(metadata.ix.total_amount, mint_info.decimals.into()),
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
pub fn withdraw(program_id: &Pubkey, acc: WithdrawAccounts, amount: u64) -> ProgramResult {
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
        //TODO: Update in future releases based on `is_withdrawal_public`
        || acc.withdraw_authority.key != acc.recipient.key
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !acc.withdraw_authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut data = acc.metadata.try_borrow_mut_data()?;
    // This thing is nasty lol
    let mut metadata: TokenStreamData = match solana_borsh::try_from_slice_unchecked(&data) {
        Ok(v) => v,
        Err(_) => return Err(InvalidMetaData.into()),
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

    metadata.withdrawn_amount += requested;
    metadata.last_withdrawn_at = now;
    let bytes = metadata.try_to_vec()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    // Return rent when everything is withdrawn
    if metadata.withdrawn_amount == metadata.ix.total_amount {
        if !acc.sender.is_writable || acc.sender.key != &metadata.sender {
            return Err(ProgramError::InvalidAccountData);
        }
        //TODO: Close metadata account once there is alternative storage solution for historic data.
        // let rent = acc.metadata.lamports();
        // **acc.metadata.try_borrow_mut_lamports()? -= rent;
        // **acc.sender.try_borrow_mut_lamports()? += rent;

        let escrow_tokens_rent = acc.escrow_tokens.lamports();
        //Close escrow token account
        msg!(
            "Returning {} lamports (rent) to {}",
            escrow_tokens_rent,
            acc.sender.key
        );
        invoke_signed(
            &spl_token::instruction::close_account(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.sender.key,
                acc.escrow_tokens.key,
                &[],
            )?,
            &[
                acc.escrow_tokens.clone(),
                acc.sender.clone(),
                acc.escrow_tokens.clone(),
            ],
            &[&seeds],
        )?;
    }

    msg!(
        "Withdrawn: {} {} tokens",
        encode_base10(requested, mint_info.decimals.into()),
        metadata.mint
    );
    msg!(
        "Remaining: {} {} tokens",
        encode_base10(
            metadata.ix.total_amount - metadata.withdrawn_amount,
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
pub fn cancel(program_id: &Pubkey, acc: CancelAccounts) -> ProgramResult {
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

    let mut data = acc.metadata.try_borrow_mut_data()?;
    let mut metadata = match TokenStreamData::try_from_slice(&data) {
        // let mut metadata: TokenStreamData = match solana_borsh::try_from_slice_unchecked(&data) {
        Ok(v) => v,
        Err(_) => return Err(InvalidMetaData.into()),
    };
    let mint_info = unpack_mint_account(&acc.mint)?;

    let now = Clock::get()?.unix_timestamp as u64;
    // if stream expired anyone can close it, if not check cancel authority
    if now < metadata.closable_at {
        //TODO: Update in future releases based on `cancelable_by_sender/recipient`
        if acc.cancel_authority.key != acc.sender.key {
            return Err(ProgramError::InvalidAccountData);
        }
        if !acc.cancel_authority.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }
    }

    if acc.sender.key != &metadata.sender
        || acc.sender_tokens.key != &metadata.sender_tokens
        || acc.recipient.key != &metadata.recipient
        || acc.recipient_tokens.key != &metadata.recipient_tokens
        || acc.mint.key != &metadata.mint
        || acc.escrow_tokens.key != &metadata.escrow_tokens
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let available = metadata.available(now);
    msg!("Available {}", available);
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
    let escrow_token_info = unpack_token_account(&acc.escrow_tokens)?;
    msg!("Amount {}", escrow_token_info.amount);
    metadata.withdrawn_amount += available;
    let remains = metadata.ix.deposited_amount - metadata.withdrawn_amount;
    msg!(
        "Deposited {} , withdrawn: {}, tokens remain {}",
        metadata.ix.deposited_amount,
        metadata.withdrawn_amount,
        remains
    );
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
    let rent_escrow_tokens = acc.escrow_tokens.lamports();
    // let remains_meta = acc.metadata.lamports();
    //Close escrow token account
    invoke_signed(
        &spl_token::instruction::close_account(
            acc.token_program.key,
            acc.escrow_tokens.key,
            acc.sender.key,
            acc.escrow_tokens.key,
            &[],
        )?,
        &[
            acc.escrow_tokens.clone(),
            acc.sender.clone(),
            acc.escrow_tokens.clone(),
        ],
        &[&seeds],
    )?;

    //TODO: Close metadata account once there is alternative storage solution for historic data.
    if now < metadata.closable_at {
        metadata.last_withdrawn_at = now;
        metadata.canceled_at = now;
    }
    // Write the metadata to the account
    let bytes = metadata.try_to_vec().unwrap();
    data[0..bytes.len()].clone_from_slice(&bytes);

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
        "Returned rent: {} lamports",
        rent_escrow_tokens /* + remains_meta */
    );

    Ok(())
}

pub fn transfer_recipient(program_id: &Pubkey, acc: TransferAccounts) -> ProgramResult {
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
    let mut metadata = match TokenStreamData::try_from_slice(&data) {
        Ok(v) => v,
        Err(_) => return Err(InvalidMetaData.into()),
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

    let bytes = metadata.try_to_vec()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    Ok(())
}

/// Top up the SPL Token stream
///
/// The function will add the amount to the metadata SPL account
pub fn topup_stream(acc: TopUpAccounts, amount: u64) -> ProgramResult {
    // Negative amount would be a problem (public function) and 0 doesn't change anything
    if amount <= 0 {
        return Err(ProgramError::InvalidArgument);
    }

    msg!("Topping up the escrow account");
    if acc.metadata.data_is_empty() || acc.escrow_tokens.owner != &spl_token::id() {
        return Err(ProgramError::UninitializedAccount);
    }

    let mut data = acc.metadata.try_borrow_mut_data()?;
    // Take metadata
    let mut metadata: TokenStreamData = match solana_borsh::try_from_slice_unchecked(&data) {
        Ok(v) => v,
        // TODO: Add "Invalid Metadata" as error
        Err(_) => return Err(ProgramError::Custom(2)),
    };

    msg!("Transferring to the escrow account");
    invoke(
        &spl_token::instruction::transfer(
            acc.token_program.key,
            acc.sender_tokens.key,
            acc.escrow_tokens.key,
            acc.sender.key,
            &[],
            amount,
        )?,
        &[
            acc.sender_tokens.clone(),
            acc.escrow_tokens.clone(),
            acc.sender.clone(),
            acc.token_program.clone(),
        ],
    )?;
    // Update metadata deposited amount
    metadata.ix.deposited_amount += amount;
    // Update closable_at
    metadata.closable_at = metadata.closable();
    // Write the metadata to the account
    let bytes = metadata.try_to_vec().unwrap();
    data[0..bytes.len()].clone_from_slice(&bytes);

    let mint_info = unpack_mint_account(&acc.mint)?;
    msg!(
        "Successfully topped up {} to token stream {} on behalf of {}",
        encode_base10(amount, mint_info.decimals.into()),
        acc.escrow_tokens.key,
        acc.sender.key,
    );

    Ok(())
}

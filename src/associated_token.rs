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
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    system_instruction, system_program,
    sysvar::{clock::Clock, fees::Fees, rent::Rent, Sysvar},
};

use crate::state::TokenStream;
use crate::utils::{duration_sanity, unpack_mint_account, unpack_token_account};

/// Initializes an SPL token stream
///
/// The account order:
///
///
/// The function shall initialize new accounts to hold the tokens,
/// and the stream's metadata. Both accounts will be funded to be
/// rent-exempt if necessary. When the stream is finished, these
/// shall be returned to the stream initializer.
pub fn initialize_token_stream(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    mut metadata: TokenStream,
) -> ProgramResult {
    msg!("Initializing SPL token stream");
    let account_info_iter = &mut accounts.iter();
    let sender_wallet = next_account_info(account_info_iter)?;
    let sender_tokens = next_account_info(account_info_iter)?;
    //let recipient_wallet = next_account_info(account_info_iter)?;
    let recipient_tokens = next_account_info(account_info_iter)?;
    let metadata_account = next_account_info(account_info_iter)?;
    let escrow_account = next_account_info(account_info_iter)?;
    let mint_account = next_account_info(account_info_iter)?;
    let self_program = next_account_info(account_info_iter)?;
    let token_program_account = next_account_info(account_info_iter)?;
    let system_program_account = next_account_info(account_info_iter)?;

    spl_token::check_program_account(&token_program_account.key)?;
    if self_program.key != program_id || !system_program::check_id(&system_program_account.key) {
        return Err(ProgramError::InvalidAccountData);
    }

    if !metadata_account.data_is_empty() || !escrow_account.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    if !sender_wallet.is_writable
        || !sender_tokens.is_writable
        || !metadata_account.is_writable
        || !escrow_account.is_writable
        || !recipient_tokens.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if !sender_wallet.is_signer || !metadata_account.is_signer || !escrow_account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let sender_token_info = unpack_token_account(sender_tokens)?;
    let mint_info = unpack_mint_account(mint_account)?;
    if !mint_info.is_initialized {
        return Err(ProgramError::InvalidAccountData);
    }

    // We also transfer enough to be rent-exempt on the metadata account.
    // After all funds are unlocked and withdrawn, the remains are
    // returned to the sender's account.
    let metadata_struct_size = std::mem::size_of::<TokenStream>();
    let tokens_struct_size = spl_token::state::Account::LEN;
    let cluster_rent = Rent::get()?;
    let metadata_rent = cluster_rent.minimum_balance(metadata_struct_size);
    let tokens_rent = cluster_rent.minimum_balance(tokens_struct_size);
    // This is the serialized metadata we will write into the escrow.
    metadata.withdrawn = 0;
    let bytes = bincode::serialize(&metadata).unwrap();

    // Fee calculator
    let fees = Fees::get()?;
    let lps = fees.fee_calculator.lamports_per_signature;

    // tokens_rent*2 is so we're sure we can fund recipient_tokens account.
    if sender_wallet.lamports() < metadata_rent + tokens_rent * 2 + (4 * lps) {
        return Err(ProgramError::InsufficientFunds);
    }

    if sender_token_info.amount <= metadata.amount {
        return Err(ProgramError::InsufficientFunds);
    }

    let now = Clock::get()?.unix_timestamp as u64;
    if !duration_sanity(now, metadata.start_time, metadata.end_time) {
        return Err(ProgramError::InvalidArgument);
    }

    msg!("Creating metadata holding account");
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

    msg!("Creating token holding escrow account");
    invoke(
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
    )?;

    msg!(
        "Initializing escrow account for {:?} token",
        mint_account.key
    );
    invoke(
        &spl_token::instruction::initialize_account(
            token_program_account.key,
            escrow_account.key,
            mint_account.key,
            program_id,
        )?,
        &[
            token_program_account.clone(),
            escrow_account.clone(),
            mint_account.clone(),
            self_program.clone(),
        ],
    )?;

    msg!("Moving funds into escrow");
    invoke_signed(
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
        &[&[]],
    )?;

    Ok(())
}

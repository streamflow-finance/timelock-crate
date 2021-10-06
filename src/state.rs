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
use serde::{Deserialize, Serialize};
use solana_program::{account_info::AccountInfo, pubkey::Pubkey};

/// The struct containing instructions for initializing a stream
#[repr(C)]
#[derive(Deserialize, Serialize, Debug)]
pub struct StreamInstruction {
    /// Timestamp when the funds start unlocking
    pub start_time: u64,
    /// Timestamp when all funds are unlocked
    pub end_time: u64,
    /// Amount of funds locked
    pub amount: u64,
    /// Time step (period) per which vesting occurs
    pub period: u64,
    /// Vesting contract "cliff" timestamp
    pub cliff: u64,
    /// Amount unlocked at the "cliff" timestamp
    pub cliff_amount: u64,
}

/// The account-holding struct for the stream initialization instruction
#[derive(Debug)]
pub struct InitializeAccounts<'a> {
    /// The main wallet address of the initializer
    pub sender_wallet: AccountInfo<'a>,
    /// The associated token account address of `sender_wallet`
    pub sender_tokens: AccountInfo<'a>,
    /// The main wallet address of the recipient
    pub recipient_wallet: AccountInfo<'a>,
    /// The associated token account address of `recipient_wallet` (could be either empty or initialized)
    pub recipient_tokens: AccountInfo<'a>,
    /// The account holding the stream metadata — expects empty (not-initialized) account
    pub metadata_account: AccountInfo<'a>,
    /// The escrow account holding the stream funds — expects empty (not-initialized) account
    pub escrow_account: AccountInfo<'a>,
    /// The SPL token mint account
    pub mint_account: AccountInfo<'a>,
    /// The Rent Sysvar account
    pub rent_account: AccountInfo<'a>,
    /// The SPL token program
    pub token_program_account: AccountInfo<'a>,
    /// The Solana system program
    pub system_program_account: AccountInfo<'a>,
}

/// The account-holding struct for the stream withdraw instruction
pub struct WithdrawAccounts<'a> {
    pub recipient_wallet: AccountInfo<'a>,
    /// The associated token account address of `recipient_wallet`
    pub recipient_tokens: AccountInfo<'a>,
    /// The account holding the stream metadata
    pub metadata_account: AccountInfo<'a>,
    /// The escrow account holding the stream funds
    pub escrow_account: AccountInfo<'a>,
    /// The SPL token mint account
    //todo: needed only for logging/debugging purposes, to get the token decimals
    pub mint_account: AccountInfo<'a>,
    /// The SPL token program
    pub token_program_account: AccountInfo<'a>,
}

/// The account-holding struct for the stream cancel instruction
pub struct CancelAccounts<'a> {
    /// The main wallet address of the initializer
    pub sender_wallet: AccountInfo<'a>,
    /// The associated token account address of `sender_wallet`
    pub sender_tokens: AccountInfo<'a>,
    /// The main wallet address of the recipient
    pub recipient_wallet: AccountInfo<'a>,
    /// The associated token account address of `recipient_wallet`
    pub recipient_tokens: AccountInfo<'a>,
    /// The account holding the stream metadata
    pub metadata_account: AccountInfo<'a>,
    /// The escrow account holding the stream funds
    pub escrow_account: AccountInfo<'a>,
    /// The SPL token mint account
    //todo: needed only for logging/debugging purposes, to get the token decimals
    pub mint_account: AccountInfo<'a>,
    /// The SPL token program
    pub token_program_account: AccountInfo<'a>,
}

/// Accounts needed for updating stream recipient
pub struct TransferAccounts<'a> {
    /// Wallet address of the existing recipient
    pub existing_recipient_wallet: AccountInfo<'a>,
    /// New stream beneficiary
    pub new_recipient_wallet: AccountInfo<'a>,
    /// New stream beneficiary's token account. If not initialized, it will be created and `existing_recipient_wallet` is a fee payer
    pub new_recipient_tokens: AccountInfo<'a>,
    /// The account holding the stream metadata
    pub metadata_account: AccountInfo<'a>,
    /// The escrow account holding the stream funds
    pub escrow_account: AccountInfo<'a>,
    /// The SPL token mint account needed in case associated account for the new recipients is being created
    pub mint_account: AccountInfo<'a>,
    /// Rent account
    pub rent_account: AccountInfo<'a>,
    /// The Associated Token program needed in case associated account for the new recipients is being created
    pub token_program_account: AccountInfo<'a>,
    /// The Solana system program needed in case associated account for the new recipients is being created
    pub system_program_account: AccountInfo<'a>,
}

/// TokenStreamData is the struct containing metadata for an SPL token stream.
#[repr(C)]
#[derive(Deserialize, Serialize, Debug)]
pub struct TokenStreamData {
    /// The stream instruction
    pub ix: StreamInstruction,
    /// Amount of funds withdrawn
    pub withdrawn: u64,
    /// Pubkey of the stream initializer
    pub sender_wallet: Pubkey,
    /// Pubkey of the stream initializer's token account
    pub sender_tokens: Pubkey,
    /// Pubkey of the stream recipient
    pub recipient_wallet: Pubkey,
    /// Pubkey of the stream recipient's token account
    pub recipient_tokens: Pubkey,
    /// Pubkey of the token mint
    pub mint: Pubkey,
    /// Pubkey of the account holding the locked tokens
    pub escrow: Pubkey,
}

#[allow(clippy::too_many_arguments)]
impl TokenStreamData {
    /// Initialize a new `TokenStreamData` struct.
    pub fn new(
        start_time: u64,
        end_time: u64,
        amount: u64,
        period: u64,
        cliff: u64,
        cliff_amount: u64,
        sender_wallet: Pubkey,
        sender_tokens: Pubkey,
        recipient_wallet: Pubkey,
        recipient_tokens: Pubkey,
        mint: Pubkey,
        escrow: Pubkey,
    ) -> Self {
        let ix = StreamInstruction {
            start_time,
            end_time,
            amount,
            period,
            cliff,
            cliff_amount,
        };

        Self {
            ix,
            withdrawn: 0,
            sender_wallet,
            sender_tokens,
            recipient_wallet,
            recipient_tokens,
            mint,
            escrow,
        }
    }

    /// Calculate amount available for withdrawal with given timestamp.
    pub fn available(&self, now: u64) -> u64 {
        if self.ix.start_time > now || self.ix.cliff > now {
            return 0;
        }

        if now >= self.ix.end_time {
            return self.ix.amount - self.withdrawn;
        }

        let cliff = if self.ix.cliff > 0 {
            self.ix.cliff
        } else {
            self.ix.start_time
        };

        let cliff_amount = if self.ix.cliff_amount > 0 {
            self.ix.cliff_amount
        } else {
            0
        };

        //TODO: use uint arithmetics, floats are imprecise
        let num_periods = (self.ix.end_time - cliff) as f64 / self.ix.period as f64;
        let period_amount = (self.ix.amount - cliff_amount) as f64 / num_periods;
        let periods_passed = (now - cliff) / self.ix.period;
        (periods_passed as f64 * period_amount) as u64 + cliff_amount - self.withdrawn
    }
}

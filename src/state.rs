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
#[cfg(feature = "anchor-support")]
use anchor_lang::prelude::*;
#[cfg(feature = "anchor-support")]
declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{account_info::AccountInfo, pubkey::Pubkey};

/// The struct containing instructions for initializing a stream
#[repr(C)]
#[derive(BorshDeserialize, BorshSerialize, Clone, Debug)]
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

#[cfg(feature = "no-anchor-support")]
impl Default for StreamInstruction {
    fn default() -> Self {
        StreamInstruction {
            start_time: 0,
            end_time: 0,
            amount: 0,
            period: 1,
            cliff: 0,
            cliff_amount: 0,
        }
    }
}

/// The account-holding struct for the stream initialization instruction
#[derive(Debug)]
pub struct InitializeAccounts<'a> {
    /// The main wallet address of the initializer
    pub sender: AccountInfo<'a>,
    /// The associated token account address of `sender`
    pub sender_tokens: AccountInfo<'a>,
    /// The main wallet address of the recipient
    pub recipient: AccountInfo<'a>,
    /// The associated token account address of `recipient` (could be either empty or initialized)
    pub recipient_tokens: AccountInfo<'a>,
    /// The account holding the stream metadata — expects empty (non-initialized) account
    pub metadata: AccountInfo<'a>,
    /// The escrow account holding the stream funds — expects empty (non-initialized) account
    pub escrow_tokens: AccountInfo<'a>,
    /// The SPL token mint account
    pub mint: AccountInfo<'a>,
    /// The Rent Sysvar account
    pub rent: AccountInfo<'a>,
    /// The SPL program needed in case associated account for the new recipients is being created
    pub token_program: AccountInfo<'a>,
    /// The Associated Token program needed in case associated account for the new recipients is being created
    pub associated_token_program: AccountInfo<'a>,
    /// The Solana system program
    pub system_program: AccountInfo<'a>,
}

/// The account-holding struct for the stream withdraw instruction
pub struct WithdrawAccounts<'a> {
    pub recipient: AccountInfo<'a>,
    /// The associated token account address of `recipient`
    pub recipient_tokens: AccountInfo<'a>,
    /// The account holding the stream metadata
    pub metadata: AccountInfo<'a>,
    /// The escrow account holding the stream funds
    pub escrow_tokens: AccountInfo<'a>,
    /// The SPL token mint account
    pub mint: AccountInfo<'a>,
    /// The SPL token program
    pub token_program: AccountInfo<'a>,
}

/// The account-holding struct for the stream cancel instruction
pub struct CancelAccounts<'a> {
    /// The main wallet address of the initializer
    pub sender: AccountInfo<'a>,
    /// The associated token account address of `sender`
    pub sender_tokens: AccountInfo<'a>,
    /// The main wallet address of the recipient
    pub recipient: AccountInfo<'a>,
    /// The associated token account address of `recipient`
    pub recipient_tokens: AccountInfo<'a>,
    /// The account holding the stream metadata
    pub metadata: AccountInfo<'a>,
    /// The escrow account holding the stream funds
    pub escrow_tokens: AccountInfo<'a>,
    /// The SPL token mint account
    pub mint: AccountInfo<'a>,
    /// The SPL token program
    pub token_program: AccountInfo<'a>,
}

/// Accounts needed for updating stream recipient
pub struct TransferAccounts<'a> {
    /// Wallet address of the existing recipient
    pub existing_recipient: AccountInfo<'a>,
    /// New stream beneficiary
    pub new_recipient: AccountInfo<'a>,
    /// New stream beneficiary's token account
    /// If not initialized, it will be created and `existing_recipient` is the fee payer
    pub new_recipient_tokens: AccountInfo<'a>,
    /// The account holding the stream metadata
    pub metadata: AccountInfo<'a>,
    /// The escrow account holding the stream funds
    pub escrow_tokens: AccountInfo<'a>,
    /// The SPL token mint account
    pub mint: AccountInfo<'a>,
    /// Rent account
    pub rent: AccountInfo<'a>,
    /// The SPL program needed in case associated account for the new recipients is being created
    pub token_program: AccountInfo<'a>,
    /// The Associated Token program needed in case associated account for the new recipients is being created
    pub associated_token_program: AccountInfo<'a>,
    /// The Solana system program needed in case associated account for the new recipients is being created
    pub system_program: AccountInfo<'a>,
}

/// TokenStreamData is the struct containing metadata for an SPL token stream.
#[cfg_attr(feature = "anchor-support", account)]
#[cfg_attr(
    feature = "no-anchor-support",
    derive(BorshSerialize, BorshDeserialize, Default, Debug)
)]
#[repr(C)]
pub struct TokenStreamData {
    /// The stream instruction
    pub ix: StreamInstruction,
    /// Amount of funds withdrawn
    pub withdrawn: u64,
    /// Pubkey of the stream initializer
    pub sender: Pubkey,
    /// Pubkey of the stream initializer's token account
    pub sender_tokens: Pubkey,
    /// Pubkey of the stream recipient
    pub recipient: Pubkey,
    /// Pubkey of the stream recipient's token account
    pub recipient_tokens: Pubkey,
    /// Pubkey of the token mint
    pub mint: Pubkey,
    /// Pubkey of the account holding the locked tokens
    pub escrow_tokens: Pubkey,
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
        sender: Pubkey,
        sender_tokens: Pubkey,
        recipient: Pubkey,
        recipient_tokens: Pubkey,
        mint: Pubkey,
        escrow_tokens: Pubkey,
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
            sender,
            sender_tokens,
            recipient,
            recipient_tokens,
            mint,
            escrow_tokens,
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

        // TODO: Use uint arithmetics, floats are imprecise
        let num_periods = (self.ix.end_time - cliff) as f64 / self.ix.period as f64;
        let period_amount = (self.ix.amount - cliff_amount) as f64 / num_periods;
        let periods_passed = (now - cliff) / self.ix.period;
        (periods_passed as f64 * period_amount) as u64 + cliff_amount - self.withdrawn
    }
}

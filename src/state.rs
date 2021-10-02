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
use solana_program::pubkey::Pubkey;

/// TokenStreamInstruction is the struct containing instructions for
/// initializing a SOL/SPL stream.
#[repr(C)]
#[derive(Deserialize, Serialize)]
pub struct TokenStreamInstruction {
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

/// NativeStreamData is the struct containing metadata for a native SOL stream.
#[repr(C)]
#[derive(Deserialize, Serialize)]
pub struct NativeStreamData {
    /// Timestamp when the funds start unlocking
    pub start_time: u64,
    /// Timestamp when all funds are unlocked
    pub end_time: u64,
    /// Amount of funds locked
    pub amount: u64,
    /// Amount of funds withdrawn
    pub withdrawn: u64,
    /// Pubkey of the stream initializer
    pub sender: Pubkey,
    /// Pubkey of the stream recipient
    pub recipient: Pubkey,
    /// Pubkey of the escrow account holding the locked SOL.
    pub escrow: Pubkey,
    /// Time step (period) per which vesting occurs
    pub period: u64,
    /// Vesting contract "cliff" timestamp
    pub cliff: u64,
    /// Amount unlocked at the "cliff" timestamp
    pub cliff_amount: u64,
}

#[allow(clippy::too_many_arguments)]
impl NativeStreamData {
    pub fn new(
        start_time: u64,
        end_time: u64,
        amount: u64,
        sender: Pubkey,
        recipient: Pubkey,
        escrow: Pubkey,
        period: u64,
        cliff: u64,
        cliff_amount: u64,
    ) -> Self {
        Self {
            start_time,
            end_time,
            amount,
            withdrawn: 0,
            sender,
            recipient,
            escrow,
            period,
            cliff,
            cliff_amount,
        }
    }

    pub fn available(&self, now: u64) -> u64 {
        if self.start_time > now || self.cliff > now {
            return 0;
        }

        if now >= self.end_time {
            return self.amount - self.withdrawn;
        }

        let cliff = if self.cliff > 0 {
            self.cliff
        } else {
            self.start_time
        };

        let cliff_amount = if self.cliff_amount > 0 {
            self.cliff_amount
        } else {
            0
        };

        let num_periods = (self.end_time - cliff) as f64 / self.period as f64;
        let period_amount = (self.amount - cliff_amount) as f64 / num_periods;
        let periods_passed = (now - cliff) / self.period;
        (periods_passed as f64 * period_amount) as u64 + cliff_amount - self.withdrawn
    }
}

/// TokenStreamData is the struct containing metadata for an SPL token stream.
#[repr(C)]
#[derive(Deserialize, Serialize)]
pub struct TokenStreamData {
    /// Timestamp when the funds start unlocking
    pub start_time: u64,
    /// Timestamp when all funds are unlocked
    pub end_time: u64,
    /// Amount of funds locked
    pub amount: u64,
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
    pub fn new(
        start_time: u64,
        end_time: u64,
        amount: u64,
        sender_wallet: Pubkey,
        sender_tokens: Pubkey,
        recipient_wallet: Pubkey,
        recipient_tokens: Pubkey,
        mint: Pubkey,
        escrow: Pubkey,
    ) -> Self {
        Self {
            start_time,
            end_time,
            amount,
            withdrawn: 0,
            sender_wallet,
            sender_tokens,
            recipient_wallet,
            recipient_tokens,
            mint,
            escrow,
        }
    }
}

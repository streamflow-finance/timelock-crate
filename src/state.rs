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

/// NativeStreamInstruction is the struct containing instructions for
/// initializing a native SOL stream.
#[repr(C)]
#[derive(Deserialize, Serialize)]
pub struct NativeStreamInstruction {
    /// Timestamp when the funds start unlocking
    pub start_time: u64,
    /// Timestamp when all funds are unlocked
    pub end_time: u64,
    /// Amount of funds locked
    pub amount: u64,
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
}

impl NativeStreamData {
    pub fn new(
        start_time: u64,
        end_time: u64,
        amount: u64,
        sender: Pubkey,
        recipient: Pubkey,
        escrow: Pubkey,
    ) -> Self {
        Self {
            start_time,
            end_time,
            amount,
            withdrawn: 0,
            sender,
            recipient,
            escrow,
        }
    }
}

/// TokenStream is the struct containing metadata for an SPL token stream.
#[repr(C)]
#[derive(Deserialize, Serialize)]
pub struct TokenStream {
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

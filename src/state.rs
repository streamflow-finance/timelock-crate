// Copyright (c) 2021 Ivan Jelincic <parazyd@dyne.org>
//               2021 imprfekt <imprfekt@icloud.com>
//               2021 Ivan Britvic <ivbritvic@gmail.com>
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
use solana_program::{account_info::AccountInfo, msg, pubkey::Pubkey};

// Hardcoded program version
pub const PROGRAM_VERSION: u64 = 2;

/// The struct containing instructions for initializing a stream
#[derive(BorshDeserialize, BorshSerialize, Clone, Debug)]
#[repr(C)]
pub struct StreamInstruction {
    /// Timestamp when the tokens start vesting
    pub start_time: u64,
    /// Timestamp when all tokens are fully vested
    pub end_time: u64,
    /// Deposited amount of tokens (should be <= total_amount)
    pub deposited_amount: u64,
    /// Total amount of the tokens in the escrow account if
    /// contract is fully vested
    pub total_amount: u64,
    /// Time step (period) in seconds per which the vesting occurs
    pub period: u64,
    /// Vesting contract "cliff" timestamp
    pub cliff: u64,
    /// Amount unlocked at the "cliff" timestamp
    pub cliff_amount: u64,
    /// Whether or not a stream can be canceled by a sender
    pub cancelable_by_sender: bool,
    /// Whether or not a stream can be canceled by a recipient
    pub cancelable_by_recipient: bool,
    /// Whether or not a 3rd party can initiate withdraw in the name of recipient
    pub withdrawal_public: bool,
    /// Whether or not the sender can transfer the stream
    pub transferable_by_sender: bool,
    /// Whether or not the recipient can transfer the stream
    pub transferable_by_recipient: bool,
    /// Release rate of recurring payment
    pub release_rate: u64,
    /// The name of this stream
    pub stream_name: String,
}

/// TokenStreamData is the struct containing metadata for an SPL token stream.
#[derive(BorshSerialize, BorshDeserialize, Debug)]
#[repr(C)]
pub struct TokenStreamData {
    /// Magic bytes, will be used for version of the contract
    pub magic: u64,
    /// Timestamp when stream was created
    pub created_at: u64,
    /// Amount of funds withdrawn
    pub withdrawn_amount: u64,
    /// Timestamp when stream was canceled (if canceled)
    pub canceled_at: u64,
    /// Timestamp at which stream can be safely canceled by a 3rd party
    /// (Stream is either fully vested or there isn't enough capital to
    /// keep it active)
    pub closable_at: u64,
    /// Timestamp of the last withdrawal
    pub last_withdrawn_at: u64,
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
    /// Escrow account holding the locked tokens for recipient
    pub escrow_tokens: Pubkey,
    /// Streamflow treasury authority
    pub streamflow_treasury: Pubkey,
    /// Escrow account holding the locked tokens for Streamflow (fee account)
    pub streamflow_treasury_tokens: Pubkey,
    /// The total fee amount for streamflow
    pub streamflow_fee_total: u64,
    /// The withdrawn fee amount for streamflow
    pub streamflow_fee_withdrawn: u64,
    /// Streamflow partner authority
    pub partner: Pubkey,
    /// Escrow account holding the locked tokens for Streamflow partner (fee account)
    pub partner_tokens: Pubkey,
    /// The total fee amount for the partner
    pub partner_fee_total: u64,
    /// The withdrawn fee amount for the partner
    pub partner_fee_withdrawn: u64,
    /// The stream instruction
    pub ix: StreamInstruction,
}

impl TokenStreamData {
    /// Initialize a new `TokenStreamData` struct.
    pub fn new(
        now: u64,
        acc: InstructionAccounts,
        ix: StreamInstruction,
        partner_fee: u64,
        strm_fee: u64,
    ) -> Self {
        // TODO: calculate cancel_time based on other parameters (incl. deposited_amount)
        Self {
            magic: PROGRAM_VERSION,
            created_at: now, // TODO: is oke?
            withdrawn_amount: 0,
            canceled_at: 0,
            closable_at: ix.end_time, // TODO: is oke?
            last_withdrawn_at: 0,
            sender: *acc.sender.key,
            sender_tokens: *acc.sender_tokens.key,
            recipient: *acc.recipient.key,
            recipient_tokens: *acc.recipient_tokens.key,
            mint: *acc.mint.key,
            escrow_tokens: *acc.escrow_tokens.key,
            streamflow_treasury: *acc.streamflow_treasury.key,
            streamflow_treasury_tokens: *acc.streamflow_treasury_tokens.key,
            streamflow_fee_total: strm_fee,
            streamflow_fee_withdrawn: 0,
            partner: *acc.partner.key,
            partner_tokens: *acc.partner_tokens.key,
            partner_fee_total: partner_fee,
            partner_fee_withdrawn: 0,
            ix,
        }
    }

    /// Calculate timestamp when stream is closable
    /// end_time when deposit == total else time when funds run out
    pub fn closable(&self) -> u64 {
        let cliff_time = if self.ix.cliff > 0 { self.ix.cliff } else { self.ix.start_time };

        let cliff_amount = if self.ix.cliff_amount > 0 { self.ix.cliff_amount } else { 0 };
        // Deposit smaller then cliff amount, cancelable at cliff
        if self.ix.deposited_amount < cliff_amount {
            return cliff_time
        }
        // Nr of seconds after the cliff
        let seconds_nr = self.ix.end_time - cliff_time;

        let amount_per_second = if self.ix.release_rate > 0 {
            self.ix.release_rate / self.ix.period
        } else {
            // stream per second
            ((self.ix.total_amount - cliff_amount) / seconds_nr) as u64
        };
        // Seconds till account runs out of available funds, +1 as ceil (integer)
        let seconds_left = ((self.ix.deposited_amount - cliff_amount) / amount_per_second) + 1;

        msg!(
            "Release {}, Period {}, seconds left {}",
            self.ix.release_rate,
            self.ix.period,
            seconds_left
        );
        // closable_at time, ignore end_time when recurring
        if cliff_time + seconds_left > self.ix.end_time && self.ix.release_rate == 0 {
            self.ix.end_time
        } else {
            cliff_time + seconds_left
        }
    }
}
#[derive(Clone, Debug)]
pub struct InstructionAccounts<'a> {
    pub authority: AccountInfo<'a>,
    pub sender: AccountInfo<'a>,
    pub sender_tokens: AccountInfo<'a>,
    pub recipient: AccountInfo<'a>,
    pub recipient_tokens: AccountInfo<'a>,
    pub metadata: AccountInfo<'a>,
    pub escrow_tokens: AccountInfo<'a>,
    pub streamflow_treasury: AccountInfo<'a>,
    pub streamflow_treasury_tokens: AccountInfo<'a>,
    pub partner: AccountInfo<'a>,
    pub partner_tokens: AccountInfo<'a>,
    pub mint: AccountInfo<'a>,
    pub rent: AccountInfo<'a>,
    pub token_program: AccountInfo<'a>,
    pub associated_token_program: AccountInfo<'a>,
    pub system_program: AccountInfo<'a>,
}

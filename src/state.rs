use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{entrypoint::ProgramResult, pubkey::Pubkey};
use std::cell::RefMut;

use crate::{
    create::CreateAccounts,
    utils::{calculate_external_deposit, calculate_fee_from_amount},
};

// Hardcoded program version
pub const PROGRAM_VERSION: u8 = 2;
pub const STRM_TREASURY: &str = "Ht5G1RhkcKnpLVLMhqJc5aqZ4wYUEbxbtZwGCVbgU7DL"; //todo: update
pub const MAX_STRING_SIZE: usize = 64;
pub const STRM_FEE_DEFAULT_PERCENT: f32 = 0.25;
pub const ESCROW_SEED_PREFIX: &[u8] = b"strfi_escrow";

/// The struct containing instructions for initializing a stream
#[derive(BorshDeserialize, BorshSerialize, Clone, Debug)]
#[repr(C)]
pub struct CreateParams {
    /// Timestamp when the tokens start vesting
    pub start_time: u64,
    /// Deposited amount of tokens
    pub net_amount_deposited: u64,
    /// Time step (period) in seconds per which the vesting/release occurs
    pub period: u64,
    /// Amount released per period. Combined with `period`, we get a release rate.
    pub amount_per_period: u64,
    /// Vesting contract "cliff" timestamp
    pub cliff: u64,
    /// Amount unlocked at the "cliff" timestamp
    pub cliff_amount: u64,
    /// Whether or not a stream can be canceled by a sender
    pub cancelable_by_sender: bool,
    /// Whether or not a stream can be canceled by a recipient
    pub cancelable_by_recipient: bool,
    /// Whether or not a 3rd party can initiate withdraw in the name of recipient
    pub automatic_withdrawal: bool,
    /// Whether or not the sender can transfer the stream
    pub transferable_by_sender: bool,
    /// Whether or not the recipient can transfer the stream
    pub transferable_by_recipient: bool,
    /// Whether topup is enabled
    pub can_topup: bool,
    /// The name of this stream
    pub stream_name: [u8; 64],
}

impl CreateParams {
    // Calculate timestamp when stream is closable
    pub fn calculate_end_time(&self) -> u64 {
        let cliff_time = if self.cliff > 0 { self.cliff } else { self.start_time };

        let cliff_amount = self.cliff_amount;

        if self.net_amount_deposited < cliff_amount {
            return cliff_time
        }
        // Nr of periods after the cliff
        let periods_left = (self.net_amount_deposited - cliff_amount) / self.amount_per_period;

        // Seconds till account runs out of available funds, +1 as ceil (integer)
        let seconds_left = periods_left * self.period + 1;

        cliff_time + seconds_left
    }
}

/// TokenStreamData is the struct containing metadata for an SPL token stream.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
#[repr(C)]
pub struct Contract {
    /// Magic bytes
    pub magic: u64,
    /// Version of the program
    pub version: u8,
    /// Timestamp when stream was created
    pub created_at: u64,
    /// Amount of funds withdrawn
    pub amount_withdrawn: u64,
    /// Timestamp when stream was canceled (if canceled)
    pub canceled_at: u64,
    /// Timestamp at which stream can be safely canceled by a 3rd party
    /// (Stream is either fully vested or there isn't enough capital to
    /// keep it active)
    pub end_time: u64,
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
    /// Fee percentage for Streamflow
    pub streamflow_fee_percent: f32,
    /// Streamflow partner authority
    pub partner: Pubkey,
    /// Escrow account holding the locked tokens for Streamflow partner (fee account)
    pub partner_tokens: Pubkey,
    /// The total fee amount for the partner
    pub partner_fee_total: u64,
    /// The withdrawn fee amount for the partner
    pub partner_fee_withdrawn: u64,
    /// Fee percentage for partner
    pub partner_fee_percent: f32,
    /// The stream instruction
    pub ix: CreateParams,
}

impl Contract {
    /// Initialize a new `TokenStreamData` struct.
    pub fn new(
        now: u64,
        acc: CreateAccounts,
        ix: CreateParams,
        partner_fee_total: u64,
        partner_fee_percent: f32,
        streamflow_fee_total: u64,
        streamflow_fee_percent: f32,
    ) -> Self {
        Self {
            magic: 0,
            version: PROGRAM_VERSION,
            created_at: now,
            amount_withdrawn: 0,
            canceled_at: 0,
            end_time: ix.calculate_end_time(),
            last_withdrawn_at: 0,
            sender: *acc.sender.key,
            sender_tokens: *acc.sender_tokens.key,
            recipient: *acc.recipient.key,
            recipient_tokens: *acc.recipient_tokens.key,
            mint: *acc.mint.key,
            escrow_tokens: *acc.escrow_tokens.key,
            streamflow_treasury: *acc.streamflow_treasury.key,
            streamflow_treasury_tokens: *acc.streamflow_treasury_tokens.key,
            streamflow_fee_total,
            streamflow_fee_withdrawn: 0,
            streamflow_fee_percent,
            partner: *acc.partner.key,
            partner_tokens: *acc.partner_tokens.key,
            partner_fee_total,
            partner_fee_withdrawn: 0,
            partner_fee_percent,
            ix,
        }
    }

    pub fn gross_amount(&self) -> u64 {
        self.ix.net_amount_deposited + self.streamflow_fee_total + self.partner_fee_total
    }

    pub fn sync_balance(&mut self, balance: u64) {
        let gross_amount = self.gross_amount();
        let external_deposit =
            calculate_external_deposit(balance, gross_amount, self.amount_withdrawn);

        if external_deposit > 0 {
            self.deposit(external_deposit);
        }
    }

    pub fn deposit(&mut self, gross_amount: u64) {
        let partner_fee_addition =
            calculate_fee_from_amount(gross_amount, self.partner_fee_percent);
        let strm_fee_addition = calculate_fee_from_amount(gross_amount, self.partner_fee_percent);
        self.ix.net_amount_deposited += gross_amount - partner_fee_addition - strm_fee_addition;
        self.partner_fee_total += partner_fee_addition;
        self.streamflow_fee_total += strm_fee_addition;
        self.end_time = self.ix.calculate_end_time();
    }
}

pub fn save_account_info(metadata: &Contract, mut data: RefMut<&mut [u8]>) -> ProgramResult {
    let bytes = metadata.try_to_vec()?;
    data[0..bytes.len()].clone_from_slice(&bytes);
    Ok(())
}

pub fn find_escrow_account(version: u8, seed: &[u8], pid: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[ESCROW_SEED_PREFIX, seed], pid)
}

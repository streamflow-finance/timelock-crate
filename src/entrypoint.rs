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
use std::convert::TryInto;

use borsh::BorshDeserialize;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    program_error::ProgramError,
    pubkey::Pubkey,
};

use crate::{
    cancel_stream::{cancel, CancelAccounts},
    create_stream::{create, CreateAccounts},
    state::StreamInstruction,
    topup_stream::{topup, TopupAccounts},
    transfer_recipient::{transfer_recipient, TransferAccounts},
    withdraw_stream::{withdraw, WithdrawAccounts},
};

entrypoint!(process_instruction);
pub fn process_instruction(pid: &Pubkey, acc: &[AccountInfo], ix: &[u8]) -> ProgramResult {
    let ai = &mut acc.iter();

    match ix[0] {
        0 => {
            let ia = CreateAccounts {
                sender: next_account_info(ai)?.clone(),
                sender_tokens: next_account_info(ai)?.clone(),
                recipient: next_account_info(ai)?.clone(),
                recipient_tokens: next_account_info(ai)?.clone(),
                metadata: next_account_info(ai)?.clone(),
                escrow_tokens: next_account_info(ai)?.clone(),
                streamflow_treasury: next_account_info(ai)?.clone(),
                streamflow_treasury_tokens: next_account_info(ai)?.clone(),
                partner: next_account_info(ai)?.clone(),
                partner_tokens: next_account_info(ai)?.clone(),
                mint: next_account_info(ai)?.clone(),
                fee_oracle: next_account_info(ai)?.clone(),
                rent: next_account_info(ai)?.clone(),
                token_program: next_account_info(ai)?.clone(),
                associated_token_program: next_account_info(ai)?.clone(),
                system_program: next_account_info(ai)?.clone(),
            };
            let si = StreamInstruction::try_from_slice(&ix[1..])?;
            return create(pid, ia, si)
        }
        1 => {
            let ia = WithdrawAccounts {
                authority: next_account_info(ai)?.clone(),
                sender: next_account_info(ai)?.clone(),
                sender_tokens: next_account_info(ai)?.clone(),
                recipient: next_account_info(ai)?.clone(),
                recipient_tokens: next_account_info(ai)?.clone(),
                metadata: next_account_info(ai)?.clone(),
                escrow_tokens: next_account_info(ai)?.clone(),
                streamflow_treasury: next_account_info(ai)?.clone(),
                streamflow_treasury_tokens: next_account_info(ai)?.clone(),
                partner: next_account_info(ai)?.clone(),
                partner_tokens: next_account_info(ai)?.clone(),
                mint: next_account_info(ai)?.clone(),
                token_program: next_account_info(ai)?.clone(),
            };
            let amount = u64::from_le_bytes(ix[1..].try_into().unwrap());
            return withdraw(pid, ia, amount)
        }
        2 => {
            let ia = CancelAccounts {
                authority: next_account_info(ai)?.clone(),
                sender: next_account_info(ai)?.clone(),
                sender_tokens: next_account_info(ai)?.clone(),
                recipient: next_account_info(ai)?.clone(),
                recipient_tokens: next_account_info(ai)?.clone(),
                metadata: next_account_info(ai)?.clone(),
                escrow_tokens: next_account_info(ai)?.clone(),
                streamflow_treasury: next_account_info(ai)?.clone(),
                streamflow_treasury_tokens: next_account_info(ai)?.clone(),
                partner: next_account_info(ai)?.clone(),
                partner_tokens: next_account_info(ai)?.clone(),
                mint: next_account_info(ai)?.clone(),
                token_program: next_account_info(ai)?.clone(),
            };
            return cancel(pid, ia)
        }
        3 => {
            let ia = TransferAccounts {
                authority: next_account_info(ai)?.clone(),
                recipient: next_account_info(ai)?.clone(),
                recipient_tokens: next_account_info(ai)?.clone(),
                metadata: next_account_info(ai)?.clone(),
                mint: next_account_info(ai)?.clone(),
                rent: next_account_info(ai)?.clone(),
                token_program: next_account_info(ai)?.clone(),
                associated_token_program: next_account_info(ai)?.clone(),
                system_program: next_account_info(ai)?.clone(),
            };
            return transfer_recipient(pid, ia)
        }
        4 => {
            let ia = TopupAccounts {
                sender: next_account_info(ai)?.clone(),
                sender_tokens: next_account_info(ai)?.clone(),
                metadata: next_account_info(ai)?.clone(),
                escrow_tokens: next_account_info(ai)?.clone(),
                streamflow_treasury: next_account_info(ai)?.clone(),
                streamflow_treasury_tokens: next_account_info(ai)?.clone(),
                partner: next_account_info(ai)?.clone(),
                partner_tokens: next_account_info(ai)?.clone(),
                mint: next_account_info(ai)?.clone(),
                token_program: next_account_info(ai)?.clone(),
            };
            let amount = u64::from_le_bytes(ix[1..].try_into().unwrap());
            return topup(pid, ia, amount)
        }
        _ => {}
    }

    Err(ProgramError::InvalidInstructionData)
}

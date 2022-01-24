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
    cancel::{cancel, CancelAccounts},
    create::{create, CreateAccounts},
    instruction::StreamInstruction,
    state::CreateParams,
    topup::{topup, TopupAccounts},
    transfer::{transfer_recipient, TransferAccounts},
    withdraw::{withdraw, WithdrawAccounts},
};

entrypoint!(process_instruction);
pub fn process_instruction(pid: &Pubkey, acc: &[AccountInfo], ix: &[u8]) -> ProgramResult {
    let ai = &mut acc.iter();
    let instruction = StreamInstruction::unpack(ix)?;

    match instruction {
        StreamInstruction::Create { create_params } => {
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
            return create(pid, ia, create_params)
        }
        StreamInstruction::Withdraw { amount } => {
            let ia = WithdrawAccounts {
                authority: next_account_info(ai)?.clone(),
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
            return withdraw(pid, ia, amount)
        }
        StreamInstruction::Cancel {} => {
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
        StreamInstruction::Transfer {} => {
            let ia = TransferAccounts {
                authority: next_account_info(ai)?.clone(),
                new_recipient: next_account_info(ai)?.clone(),
                new_recipient_tokens: next_account_info(ai)?.clone(),
                metadata: next_account_info(ai)?.clone(),
                mint: next_account_info(ai)?.clone(),
                rent: next_account_info(ai)?.clone(),
                token_program: next_account_info(ai)?.clone(),
                associated_token_program: next_account_info(ai)?.clone(),
                system_program: next_account_info(ai)?.clone(),
            };
            return transfer_recipient(pid, ia)
        }
        StreamInstruction::TopUp { amount } => {
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
            return topup(pid, ia, amount)
        }
    }
}

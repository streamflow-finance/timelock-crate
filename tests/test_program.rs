use anyhow::Result;
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    program_error::ProgramError,
    pubkey::Pubkey,
};
use solana_program_test::{processor, tokio, ProgramTest};
use solana_sdk::{signature::Signer, signer::keypair::Keypair};
use std::convert::TryInto;

use streamflow_timelock::state::{
    CancelAccounts, InitializeAccounts, StreamInstruction, TransferAccounts, WithdrawAccounts,
};
use streamflow_timelock::token::{cancel, create, transfer_recipient, withdraw};

#[derive(BorshSerialize, BorshDeserialize, Clone)]
struct CreateStreamIx {
    ix: u8,
    metadata: StreamInstruction,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
struct WithdrawStreamIx {
    ix: u8,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
struct CancelStreamIx {
    ix: u8,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
struct TransferRecipientIx {
    ix: u8,
}

entrypoint!(process_instruction);
fn process_instruction(pid: &Pubkey, acc: &[AccountInfo], ix: &[u8]) -> ProgramResult {
    let ai = &mut acc.iter();

    match ix[0] {
        0 => {
            let ia = InitializeAccounts {
                sender: next_account_info(ai)?.clone(),
                sender_tokens: next_account_info(ai)?.clone(),
                recipient: next_account_info(ai)?.clone(),
                recipient_tokens: next_account_info(ai)?.clone(),
                metadata: next_account_info(ai)?.clone(),
                escrow_tokens: next_account_info(ai)?.clone(),
                mint: next_account_info(ai)?.clone(),
                rent: next_account_info(ai)?.clone(),
                token_program: next_account_info(ai)?.clone(),
                associated_token_program: next_account_info(ai)?.clone(),
                system_program: next_account_info(ai)?.clone(),
            };

            let si = StreamInstruction::try_from_slice(&ix[1..])?;

            create(pid, ia, si)?
        }
        1 => {
            let wa = WithdrawAccounts {
                withdraw_authority: next_account_info(ai)?.clone(),
                sender: next_account_info(ai)?.clone(),
                recipient: next_account_info(ai)?.clone(),
                recipient_tokens: next_account_info(ai)?.clone(),
                metadata: next_account_info(ai)?.clone(),
                escrow_tokens: next_account_info(ai)?.clone(),
                mint: next_account_info(ai)?.clone(),
                token_program: next_account_info(ai)?.clone(),
            };

            let amnt = u64::from_le_bytes(ix[1..].try_into().unwrap());

            withdraw(pid, wa, amnt)?
        }
        2 => {
            let ca = CancelAccounts {
                cancel_authority: next_account_info(ai)?.clone(),
                sender: next_account_info(ai)?.clone(),
                sender_tokens: next_account_info(ai)?.clone(),
                recipient: next_account_info(ai)?.clone(),
                recipient_tokens: next_account_info(ai)?.clone(),
                metadata: next_account_info(ai)?.clone(),
                escrow_tokens: next_account_info(ai)?.clone(),
                mint: next_account_info(ai)?.clone(),
                token_program: next_account_info(ai)?.clone(),
            };

            cancel(pid, ca)?
        }
        3 => {
            let ta = TransferAccounts {
                existing_recipient: next_account_info(ai)?.clone(),
                new_recipient: next_account_info(ai)?.clone(),
                new_recipient_tokens: next_account_info(ai)?.clone(),
                metadata: next_account_info(ai)?.clone(),
                escrow_tokens: next_account_info(ai)?.clone(),
                mint: next_account_info(ai)?.clone(),
                rent: next_account_info(ai)?.clone(),
                token_program: next_account_info(ai)?.clone(),
                associated_token_program: next_account_info(ai)?.clone(),
                system_program: next_account_info(ai)?.clone(),
            };

            transfer_recipient(pid, ta)?
        }
        _ => {}
    }

    Err(ProgramError::InvalidInstructionData)
}

#[tokio::test]
async fn test_program() -> Result<()> {
    let program_kp = Keypair::new();
    let program_id = program_kp.pubkey();

    let mut runtime = ProgramTest::default();
    runtime.add_program(
        "streamflow_timelock",
        program_id,
        processor!(process_instruction),
    );

    let (mut banks_client, payer, recent_blockhash) = runtime.start().await;

    Ok(())
}

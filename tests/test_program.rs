use anyhow::Result;
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    program_error::ProgramError,
    pubkey::Pubkey,
    system_instruction,
};
use solana_program_test::{processor, tokio, ProgramTest};
use solana_sdk::{
    account::Account, native_token::sol_to_lamports, program_pack::Pack, signature::Signer,
    signer::keypair::Keypair, transaction::Transaction,
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
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

    let alice = Keypair::new();
    runtime.add_account(
        alice.pubkey(),
        Account {
            lamports: sol_to_lamports(1.0),
            ..Account::default()
        },
    );

    let bob = Keypair::new();
    let charlie = Keypair::new();

    let (mut banks_client, payer, recent_blockhash) = runtime.start().await;

    // Create a new SPL token
    let rent = banks_client.get_rent().await?;
    let strm_token_mint = Keypair::new();

    // Build a transaction to initialize our mint.
    let mut tx = Transaction::new_with_payer(
        &[
            system_instruction::create_account(
                &payer.pubkey(),
                &strm_token_mint.pubkey(),
                rent.minimum_balance(spl_token::state::Mint::LEN),
                spl_token::state::Mint::LEN as u64,
                &spl_token::id(),
            ),
            spl_token::instruction::initialize_mint(
                &spl_token::id(),
                &strm_token_mint.pubkey(),
                &payer.pubkey(),
                None,
                8,
            )?,
        ],
        Some(&payer.pubkey()),
    );
    tx.sign(&[&payer, &strm_token_mint], recent_blockhash);
    banks_client.process_transaction(tx).await?;

    // Once that is done, let's mint some to Alice.
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let mut tx = Transaction::new_with_payer(
        &[
            create_associated_token_account(
                &payer.pubkey(),
                &alice.pubkey(),
                &strm_token_mint.pubkey(),
            ),
            spl_token::instruction::mint_to(
                &spl_token::id(),
                &strm_token_mint.pubkey(),
                &alice_ass_token,
                &payer.pubkey(),
                &[],
                spl_token::ui_amount_to_amount(100.0, 8),
            )?,
        ],
        Some(&payer.pubkey()),
    );
    tx.sign(&[&payer], recent_blockhash);
    banks_client.process_transaction(tx).await?;

    let alice_ass_account = banks_client.get_account(alice_ass_token).await?;
    let alice_ass_account = alice_ass_account.unwrap();
    let token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(token_data.mint, strm_token_mint.pubkey());
    assert_eq!(token_data.owner, alice.pubkey());
    assert_eq!(token_data.amount, spl_token::ui_amount_to_amount(100.0, 8));

    Ok(())
}

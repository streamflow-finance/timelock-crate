use anyhow::Result;
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey,
};
use solana_program_test::{processor, tokio, ProgramTest};
use solana_sdk::{signature::Signer, signer::keypair::Keypair};

use streamflow_timelock::state::StreamInstruction;

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
    match ix[0] {
        0 => {}
        1 => {}
        2 => {}
        3 => {}
        _ => {}
    }

    Ok(())
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

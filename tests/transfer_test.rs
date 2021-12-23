use anyhow::Result;
use borsh::BorshSerialize;
use solana_program::program_error::ProgramError;
use solana_program_test::tokio;
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction},
    native_token::sol_to_lamports,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::Signer,
    signer::keypair::Keypair,
    system_program,
    sysvar::rent,
};
use spl_associated_token_account::get_associated_token_address;
use test_sdk::tools::clone_keypair;

use streamflow_timelock::state::{Contract, CreateStreamParams, PROGRAM_VERSION};

mod fascilities;

use fascilities::*;

#[tokio::test]
async fn test_sender_not_transferable_should_not_be_transfered() -> Result<()> {
    let alice = Account { lamports: sol_to_lamports(1.0), ..Account::default() };
    let bob = Account { lamports: sol_to_lamports(1.0), ..Account::default() };

    let mut tt = TimelockProgramTest::start_new(&[alice, bob]).await;

    let alice = clone_keypair(&tt.accounts[0]);
    let bob = clone_keypair(&tt.accounts[1]);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());

    tt.bench.create_mint(&strm_token_mint, &tt.bench.payer.pubkey()).await;

    tt.bench.create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey()).await;

    tt.bench
        .mint_tokens(
            &strm_token_mint.pubkey(),
            &payer,
            &alice_ass_token,
            spl_token::ui_amount_to_amount(100.0, 8),
        )
        .await;

    let alice_ass_account = tt.bench.get_account(&alice_ass_token).await.unwrap();
    let alice_token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(alice_token_data.amount, spl_token::ui_amount_to_amount(100.0, 8));
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let metadata_kp = Keypair::new();
    let (escrow_tokens_pubkey, _) =
        Pubkey::find_program_address(&[metadata_kp.pubkey().as_ref()], &tt.program_id);

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;

    let transferable_by_sender = false;

    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateStreamParams {
            start_time: now + 10,
            end_time: now + 1010,
            deposited_amount: spl_token::ui_amount_to_amount(10.0, 8),
            period: 1,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            withdrawal_public: false,
            transferable_by_sender,
            transferable_by_recipient: false,
            release_rate: 0,
            stream_name: "TransferStream".to_string(),
        },
    };

    let create_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &create_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), true),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );

    tt.bench.process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp])).await?;

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;

    assert_eq!(metadata_data.ix.stream_name, "TransferStream".to_string());
    assert!(metadata_data.ix.transferable_by_sender == transferable_by_sender);

    // Test if recipient can be transfered
    let transfer_ix = TransferIx { ix: 3 }; // 3 => entrypoint transfer recipient
    let transfer_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &transfer_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true),   // sender
            AccountMeta::new(alice.pubkey(), false),  // New recipient
            AccountMeta::new(alice_ass_token, false), // New recipient token account
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let transaction = tt.bench.process_transaction(&[transfer_ix_bytes], Some(&[&alice])).await;
    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;

    assert_eq!(transaction.is_err(), !transferable_by_sender);
    if transferable_by_sender {
        assert_eq!(metadata_data.recipient, alice.pubkey());
        assert_eq!(metadata_data.recipient_tokens, alice_ass_token);
    }

    Ok(())
}

#[tokio::test]
async fn test_sender_transferable_should_be_transfered() -> Result<()> {
    let alice = Account { lamports: sol_to_lamports(1.0), ..Account::default() };
    let bob = Account { lamports: sol_to_lamports(1.0), ..Account::default() };

    let mut tt = TimelockProgramTest::start_new(&[alice, bob]).await;

    let alice = clone_keypair(&tt.accounts[0]);
    let bob = clone_keypair(&tt.accounts[1]);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());

    tt.bench.create_mint(&strm_token_mint, &tt.bench.payer.pubkey()).await;

    tt.bench.create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey()).await;

    tt.bench
        .mint_tokens(
            &strm_token_mint.pubkey(),
            &payer,
            &alice_ass_token,
            spl_token::ui_amount_to_amount(100.0, 8),
        )
        .await;

    let alice_ass_account = tt.bench.get_account(&alice_ass_token).await.unwrap();
    let alice_token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(alice_token_data.amount, spl_token::ui_amount_to_amount(100.0, 8));
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let metadata_kp = Keypair::new();
    let (escrow_tokens_pubkey, _) =
        Pubkey::find_program_address(&[metadata_kp.pubkey().as_ref()], &tt.program_id);

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;

    let transferable_by_sender = true;

    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateStreamParams {
            start_time: now + 10,
            end_time: now + 1010,
            deposited_amount: spl_token::ui_amount_to_amount(10.0, 8),
            period: 1,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            withdrawal_public: false,
            transferable_by_sender,
            transferable_by_recipient: false,
            release_rate: 0,
            stream_name: "TransferStream".to_string(),
        },
    };

    let create_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &create_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), true),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );

    tt.bench.process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp])).await?;

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;

    assert_eq!(metadata_data.ix.stream_name, "TransferStream".to_string());
    assert!(metadata_data.ix.transferable_by_sender == transferable_by_sender);

    // Test if recipient can be transfered
    let transfer_ix = TransferIx { ix: 3 }; // 3 => entrypoint transfer recipient
    let transfer_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &transfer_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true),   // sender
            AccountMeta::new(alice.pubkey(), false),  // New recipient
            AccountMeta::new(alice_ass_token, false), // New recipient token account
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let transaction = tt.bench.process_transaction(&[transfer_ix_bytes], Some(&[&alice])).await;
    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;

    assert_eq!(transaction.is_err(), !transferable_by_sender);
    if transferable_by_sender {
        assert_eq!(metadata_data.recipient, alice.pubkey());
        assert_eq!(metadata_data.recipient_tokens, alice_ass_token);
    }

    Ok(())
}

#[tokio::test]
async fn test_recipient_not_transferable_should_not_be_transfered() -> Result<()> {
    let alice = Account { lamports: sol_to_lamports(1.0), ..Account::default() };
    let bob = Account { lamports: sol_to_lamports(1.0), ..Account::default() };

    let mut tt = TimelockProgramTest::start_new(&[alice, bob]).await;

    let alice = clone_keypair(&tt.accounts[0]);
    let bob = clone_keypair(&tt.accounts[1]);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());

    tt.bench.create_mint(&strm_token_mint, &tt.bench.payer.pubkey()).await;

    tt.bench.create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey()).await;

    tt.bench
        .mint_tokens(
            &strm_token_mint.pubkey(),
            &payer,
            &alice_ass_token,
            spl_token::ui_amount_to_amount(100.0, 8),
        )
        .await;

    let alice_ass_account = tt.bench.get_account(&alice_ass_token).await.unwrap();
    let alice_token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(alice_token_data.amount, spl_token::ui_amount_to_amount(100.0, 8));
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let metadata_kp = Keypair::new();
    let (escrow_tokens_pubkey, _) =
        Pubkey::find_program_address(&[metadata_kp.pubkey().as_ref()], &tt.program_id);

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;

    let transferable_by_recipient = false;

    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateStreamParams {
            start_time: now + 10,
            end_time: now + 1010,
            deposited_amount: spl_token::ui_amount_to_amount(10.0, 8),
            period: 1,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            withdrawal_public: false,
            transferable_by_sender: false,
            transferable_by_recipient,
            release_rate: 0,
            stream_name: "TransferStream".to_string(),
        },
    };

    let create_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &create_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), true),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );

    tt.bench.process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp])).await?;

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;

    assert_eq!(metadata_data.ix.stream_name, "TransferStream".to_string());
    assert!(metadata_data.ix.transferable_by_recipient == transferable_by_recipient);

    // Test if recipient can be transfered
    let transfer_ix = TransferIx { ix: 3 }; // 3 => entrypoint transfer recipient
    let transfer_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &transfer_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true),     // recipient
            AccountMeta::new(alice.pubkey(), false),  // New recipient
            AccountMeta::new(alice_ass_token, false), // New recipient token account
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let transaction = tt.bench.process_transaction(&[transfer_ix_bytes], Some(&[&bob])).await;
    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;

    assert_eq!(transaction.is_err(), !transferable_by_recipient);
    if transferable_by_recipient {
        assert_eq!(metadata_data.recipient, alice.pubkey());
        assert_eq!(metadata_data.recipient_tokens, alice_ass_token);
    }

    Ok(())
}

#[tokio::test]
async fn test_recipient_transferable_should_be_transfered() -> Result<()> {
    let alice = Account { lamports: sol_to_lamports(1.0), ..Account::default() };
    let bob = Account { lamports: sol_to_lamports(1.0), ..Account::default() };

    let mut tt = TimelockProgramTest::start_new(&[alice, bob]).await;

    let alice = clone_keypair(&tt.accounts[0]);
    let bob = clone_keypair(&tt.accounts[1]);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());

    tt.bench.create_mint(&strm_token_mint, &tt.bench.payer.pubkey()).await;

    tt.bench.create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey()).await;

    tt.bench
        .mint_tokens(
            &strm_token_mint.pubkey(),
            &payer,
            &alice_ass_token,
            spl_token::ui_amount_to_amount(100.0, 8),
        )
        .await;

    let alice_ass_account = tt.bench.get_account(&alice_ass_token).await.unwrap();
    let alice_token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(alice_token_data.amount, spl_token::ui_amount_to_amount(100.0, 8));
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let metadata_kp = Keypair::new();
    let (escrow_tokens_pubkey, _) =
        Pubkey::find_program_address(&[metadata_kp.pubkey().as_ref()], &tt.program_id);

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;

    let transferable_by_recipient = true;

    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateStreamParams {
            start_time: now + 10,
            end_time: now + 1010,
            deposited_amount: spl_token::ui_amount_to_amount(10.0, 8),
            period: 1,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            withdrawal_public: false,
            transferable_by_sender: false,
            transferable_by_recipient,
            release_rate: 0,
            stream_name: "TransferStream".to_string(),
        },
    };

    let create_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &create_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), true),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );

    tt.bench.process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp])).await?;

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;

    assert_eq!(metadata_data.ix.stream_name, "TransferStream".to_string());
    assert!(metadata_data.ix.transferable_by_recipient == transferable_by_recipient);

    // Test if recipient can be transfered
    let transfer_ix = TransferIx { ix: 3 }; // 3 => entrypoint transfer recipient
    let transfer_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &transfer_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true),     // recipient
            AccountMeta::new(alice.pubkey(), false),  // New recipient
            AccountMeta::new(alice_ass_token, false), // New recipient token account
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let transaction = tt.bench.process_transaction(&[transfer_ix_bytes], Some(&[&bob])).await;
    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;

    assert_eq!(transaction.is_err(), !transferable_by_recipient);
    if transferable_by_recipient {
        assert_eq!(metadata_data.recipient, alice.pubkey());
        assert_eq!(metadata_data.recipient_tokens, alice_ass_token);
    }

    Ok(())
}

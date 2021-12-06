use anyhow::Result;
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::program_error::ProgramError;
use solana_program_test::{processor, tokio};
use solana_sdk::{
    clock::UnixTimestamp,
    instruction::{AccountMeta, Instruction},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::Signer,
    signer::keypair::Keypair,
    system_program,
    sysvar::rent,
};
use spl_associated_token_account::get_associated_token_address;
use test_sdk::{tools::clone_keypair, ProgramTestBench, TestBenchProgram};

use streamflow_timelock::entrypoint::process_instruction;
use streamflow_timelock::state::{StreamInstruction, TokenStreamData, PROGRAM_VERSION};

#[derive(BorshSerialize, BorshDeserialize, Clone)]
struct CreateStreamIx {
    ix: u8,
    metadata: StreamInstruction,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
struct WithdrawStreamIx {
    ix: u8,
    amount: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
struct TopUpIx {
    ix: u8,
    amount: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
struct CancelIx {
    ix: u8,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
struct TransferIx {
    ix: u8,
}

pub struct TimelockProgramTest {
    pub bench: ProgramTestBench,
    pub program_id: Pubkey,
}

impl TimelockProgramTest {
    pub async fn start_new() -> Self {
        let program_id = Keypair::new().pubkey();

        let program = TestBenchProgram {
            program_name: "streamflow_timelock",
            program_id,
            process_instruction: processor!(process_instruction),
        };

        let bench = ProgramTestBench::start_new(&[program]).await;

        Self { bench, program_id }
    }

    pub async fn advance_clock_past_timestamp(&mut self, unix_timestamp: UnixTimestamp) {
        let mut clock = self.bench.get_clock().await;
        let mut n = 1;

        while clock.unix_timestamp <= unix_timestamp {
            // Since the exact time is not deterministic, keep wrapping by
            // arbitrary 400 slots until we pass the requested timestamp.
            self.bench
                .context
                .warp_to_slot(clock.slot + n * 400)
                .unwrap();

            n += 1;
            clock = self.bench.get_clock().await;
        }
    }
}

#[tokio::test]
async fn timelock_program_test() -> Result<()> {
    let mut tt = TimelockProgramTest::start_new().await;

    let alice = clone_keypair(&tt.bench.alice);
    let bob = clone_keypair(&tt.bench.bob);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());

    tt.bench
        .create_mint(&strm_token_mint, &tt.bench.payer.pubkey())
        .await;

    tt.bench
        .create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey())
        .await;

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
    assert_eq!(
        alice_token_data.amount,
        spl_token::ui_amount_to_amount(100.0, 8)
    );
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let metadata_kp = Keypair::new();
    let (escrow_tokens_pubkey, _) =
        Pubkey::find_program_address(&[metadata_kp.pubkey().as_ref()], &tt.program_id);

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;

    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: StreamInstruction {
            start_time: now + 5,
            end_time: now + 605,
            deposited_amount: spl_token::ui_amount_to_amount(20.0, 8),
            total_amount: spl_token::ui_amount_to_amount(20.0, 8),
            period: 1,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            withdrawal_public: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            release_rate: 0,
            stream_name: "TheTestoooooooooor".to_string(),
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

    tt.bench
        .process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp]))
        .await?;

    let metadata_acc = tt.bench.get_account(&metadata_kp.pubkey()).await.unwrap();
    let metadata_data: TokenStreamData = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;

    assert_eq!(metadata_acc.owner, tt.program_id);
    assert_eq!(metadata_data.magic, PROGRAM_VERSION);
    assert_eq!(metadata_data.withdrawn_amount, 0);
    assert_eq!(metadata_data.canceled_at, 0);
    assert_eq!(metadata_data.closable_at, now + 605);
    assert_eq!(metadata_data.last_withdrawn_at, 0);
    assert_eq!(metadata_data.sender, alice.pubkey());
    assert_eq!(metadata_data.sender_tokens, alice_ass_token);
    assert_eq!(metadata_data.recipient, bob.pubkey());
    assert_eq!(metadata_data.recipient_tokens, bob_ass_token);
    assert_eq!(metadata_data.mint, strm_token_mint.pubkey());
    assert_eq!(metadata_data.escrow_tokens, escrow_tokens_pubkey);
    assert_eq!(metadata_data.ix.start_time, now + 5);
    assert_eq!(metadata_data.ix.end_time, now + 605);
    assert_eq!(
        metadata_data.ix.deposited_amount,
        spl_token::ui_amount_to_amount(20.0, 8)
    );
    assert_eq!(
        metadata_data.ix.total_amount,
        spl_token::ui_amount_to_amount(20.0, 8)
    );
    assert_eq!(
        metadata_data.ix.stream_name,
        "TheTestoooooooooor".to_string()
    );

    // Let's warp ahead and try withdrawing some of the stream.
    tt.advance_clock_past_timestamp(now as i64 + 300).await;

    let withdraw_stream_ix = WithdrawStreamIx { ix: 1, amount: 0 };

    let withdraw_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &withdraw_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true),
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    tt.bench
        .process_transaction(&[withdraw_stream_ix_bytes], Some(&[&bob]))
        .await?;

    let metadata_data: TokenStreamData = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(metadata_data.withdrawn_amount, 1180000000);

    println!("{:#?}", metadata_data);
    Ok(())
}

#[tokio::test]
async fn timelock_program_test2() -> Result<()> {
    let mut tt = TimelockProgramTest::start_new().await;

    let alice = clone_keypair(&tt.bench.alice);
    let bob = clone_keypair(&tt.bench.bob);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());

    tt.bench
        .create_mint(&strm_token_mint, &tt.bench.payer.pubkey())
        .await;

    tt.bench
        .create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey())
        .await;

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
    assert_eq!(
        alice_token_data.amount,
        spl_token::ui_amount_to_amount(100.0, 8)
    );
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let metadata_kp = Keypair::new();
    let (escrow_tokens_pubkey, _) =
        Pubkey::find_program_address(&[metadata_kp.pubkey().as_ref()], &tt.program_id);

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;

    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: StreamInstruction {
            start_time: now + 10,
            end_time: now + 1010,
            deposited_amount: spl_token::ui_amount_to_amount(10.0, 8),
            total_amount: spl_token::ui_amount_to_amount(20.0, 8),
            period: 1,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            withdrawal_public: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            release_rate: 0, // Old contracts don't have it
            stream_name: "Test2".to_string(),
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

    tt.bench
        .process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp]))
        .await?;

    let metadata_acc = tt.bench.get_account(&metadata_kp.pubkey()).await.unwrap();
    let metadata_data: TokenStreamData = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;

    assert_eq!(metadata_acc.owner, tt.program_id);
    assert_eq!(metadata_data.closable_at, now + 510 + 1); // 1 after, like in function

    assert_eq!(metadata_data.ix.start_time, now + 10);
    assert_eq!(metadata_data.ix.end_time, now + 1010);
    assert_eq!(
        metadata_data.ix.deposited_amount,
        spl_token::ui_amount_to_amount(10.0, 8)
    );
    assert_eq!(
        metadata_data.ix.total_amount,
        spl_token::ui_amount_to_amount(20.0, 8)
    );
    assert_eq!(metadata_data.ix.stream_name, "Test2".to_string());

    // Test if recipient can be transfered, should return error
    let transfer_ix = TransferIx { ix: 3 }; // 3 => entrypoint transfer recipient
    let transfer_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &transfer_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true), // Existing recipient as signer
            AccountMeta::new(alice.pubkey(), false), // New recipient
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );

    let transaction_error = tt
        .bench
        .process_transaction(&[transfer_ix_bytes], Some(&[&bob]))
        .await;

    assert!(transaction_error.is_err());

    // Top up account with 12 and see new amount in escrow account
    let topup_ix = TopUpIx {
        ix: 4,
        amount: spl_token::ui_amount_to_amount(10.0, 8),
    }; // 4 => topup_stream
    let topupix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &topup_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );
    tt.bench
        .process_transaction(&[topupix_bytes], Some(&[&alice]))
        .await?;
    // let metadata_acc = tt.bench.get_account(&metadata_kp.pubkey()).await.unwrap();
    let metadata_data: TokenStreamData = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(
        metadata_data.ix.deposited_amount,
        spl_token::ui_amount_to_amount(20.0, 8)
    );
    // Closable to end_date, closable fn would return 1010 + 1
    assert_eq!(metadata_data.closable_at, now + 1010);

    // Warp ahead
    tt.advance_clock_past_timestamp(now as i64 + 200).await;

    let withdraw_stream_ix = WithdrawStreamIx {
        ix: 1,
        amount: spl_token::ui_amount_to_amount(30.0, 8),
    };

    let withdraw_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &withdraw_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true),
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    let transaction_error = tt
        .bench
        .process_transaction(&[withdraw_stream_ix_bytes], Some(&[&bob]))
        .await
        .err()
        .unwrap();

    assert_eq!(transaction_error, ProgramError::InvalidArgument);

    let some_other_kp = Keypair::new();
    let cancel_ix = CancelIx { ix: 2 };

    let cancel_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &cancel_ix.try_to_vec()?,
        vec![
            AccountMeta::new(some_other_kp.pubkey(), true), // RANDOM KEY
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    // It should be IA data error, stream hasn't expired
    let transaction_error = tt
        .bench
        .process_transaction(&[cancel_ix_bytes], Some(&[&some_other_kp]))
        .await
        .err()
        .unwrap();

    assert_eq!(transaction_error, ProgramError::InvalidAccountData);

    // Ahead with time, stream expired
    tt.advance_clock_past_timestamp(now as i64 + 2000).await;

    let cancel_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &cancel_ix.try_to_vec()?,
        vec![
            AccountMeta::new(some_other_kp.pubkey(), true), // RANDOM KEY
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    // Now stream should be cancelled
    tt.bench
        .process_transaction(&[cancel_ix_bytes], Some(&[&some_other_kp]))
        .await?;

    Ok(())
}

#[tokio::test]
async fn timelock_program_test_transfer() -> Result<()> {
    let mut tt = TimelockProgramTest::start_new().await;

    let alice = clone_keypair(&tt.bench.alice);
    let bob = clone_keypair(&tt.bench.bob);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());

    tt.bench
        .create_mint(&strm_token_mint, &tt.bench.payer.pubkey())
        .await;

    tt.bench
        .create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey())
        .await;

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
    assert_eq!(
        alice_token_data.amount,
        spl_token::ui_amount_to_amount(100.0, 8)
    );
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let metadata_kp = Keypair::new();
    let (escrow_tokens_pubkey, _) =
        Pubkey::find_program_address(&[metadata_kp.pubkey().as_ref()], &tt.program_id);

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;

    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: StreamInstruction {
            start_time: now + 10,
            end_time: now + 1010,
            deposited_amount: spl_token::ui_amount_to_amount(10.0, 8),
            total_amount: spl_token::ui_amount_to_amount(20.0, 8),
            period: 1,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            withdrawal_public: false,
            transferable_by_sender: false,
            transferable_by_recipient: true, // Should be possible to transfer stream
            release_rate: 0,                 // Old contracts don't have it
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

    tt.bench
        .process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp]))
        .await?;

    let metadata_data: TokenStreamData = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;

    assert_eq!(metadata_data.ix.stream_name, "TransferStream".to_string());
    assert!(metadata_data.ix.transferable_by_recipient);

    // Test if recipient can be transfered
    let transfer_ix = TransferIx { ix: 3 }; // 3 => entrypoint transfer recipient
    let transfer_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &transfer_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true), // Existing recipient as signer
            AccountMeta::new(alice.pubkey(), false), // New recipient
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
    tt.bench
        .process_transaction(&[transfer_ix_bytes], Some(&[&bob]))
        .await?;
    let metadata_data: TokenStreamData = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    // Check new recipient
    assert_eq!(metadata_data.recipient, alice.pubkey());
    // Check new recipient token account
    assert_eq!(metadata_data.recipient_tokens, alice_ass_token);

    Ok(())
}

#[tokio::test]
async fn timelock_program_test_recurring() -> Result<()> {
    let mut tt = TimelockProgramTest::start_new().await;

    let alice = clone_keypair(&tt.bench.alice);
    let bob = clone_keypair(&tt.bench.bob);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());

    tt.bench
        .create_mint(&strm_token_mint, &tt.bench.payer.pubkey())
        .await;

    tt.bench
        .create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey())
        .await;

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
    assert_eq!(
        alice_token_data.amount,
        spl_token::ui_amount_to_amount(100.0, 8)
    );
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let metadata_kp = Keypair::new();
    let (escrow_tokens_pubkey, _) =
        Pubkey::find_program_address(&[metadata_kp.pubkey().as_ref()], &tt.program_id);

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;

    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: StreamInstruction {
            start_time: now + 10,
            end_time: now + 1010,
            deposited_amount: spl_token::ui_amount_to_amount(10.0, 8),
            total_amount: spl_token::ui_amount_to_amount(20.0, 8),
            period: 200,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            withdrawal_public: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            release_rate: spl_token::ui_amount_to_amount(1.0, 8),
            stream_name: "Recurring".to_string(),
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

    tt.bench
        .process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp]))
        .await?;

    let metadata_acc = tt.bench.get_account(&metadata_kp.pubkey()).await.unwrap();
    let metadata_data: TokenStreamData = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;

    assert_eq!(metadata_acc.owner, tt.program_id);
    assert_eq!(metadata_data.closable_at, now + 10 + 2000 + 1); // 1 after, like in function
    assert_eq!(metadata_data.ix.start_time, now + 10);
    assert_eq!(metadata_data.ix.end_time, now + 1010);
    assert_eq!(
        metadata_data.ix.deposited_amount,
        spl_token::ui_amount_to_amount(10.0, 8)
    );
    assert_eq!(metadata_data.ix.stream_name, "Recurring".to_string());
    assert_eq!(metadata_data.ix.release_rate, 100000000);

    // Top up account with 12 and see new amount in escrow account
    let topup_ix = TopUpIx {
        ix: 4,
        amount: spl_token::ui_amount_to_amount(20.0, 8),
    }; // 4 => topup_stream
    let topupix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &topup_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );
    tt.bench
        .process_transaction(&[topupix_bytes], Some(&[&alice]))
        .await?;
    // let metadata_acc = tt.bench.get_account(&metadata_kp.pubkey()).await.unwrap();
    let metadata_data: TokenStreamData = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(
        metadata_data.ix.deposited_amount,
        spl_token::ui_amount_to_amount(30.0, 8)
    );
    // Closable to end_date, closable fn would return 1010 + 1
    assert_eq!(metadata_data.closable_at, now + 10 + 6000 + 1);

    let some_other_kp = Keypair::new();
    let cancel_ix = CancelIx { ix: 2 };

    let cancel_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &cancel_ix.try_to_vec()?,
        vec![
            AccountMeta::new(some_other_kp.pubkey(), true), // RANDOM KEY
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );
    // It should be IA data error, stream hasn't expired
    let transaction_error = tt
        .bench
        .process_transaction(&[cancel_ix_bytes], Some(&[&some_other_kp]))
        .await
        .err()
        .unwrap();

    assert_eq!(transaction_error, ProgramError::InvalidAccountData);

    // Try to withdraw more then due
    let withdraw_stream_ix = WithdrawStreamIx {
        ix: 1,
        amount: spl_token::ui_amount_to_amount(40.0, 8),
    };

    let withdraw_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &withdraw_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true),
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    // It should be Invalid argument error, available < requested amount for withdrawal
    let transaction_error = tt
        .bench
        .process_transaction(&[withdraw_stream_ix_bytes], Some(&[&bob]))
        .await
        .err()
        .unwrap();

    assert_eq!(transaction_error, ProgramError::InvalidArgument);

    // Ahead with time, stream expired
    // Beware test clock is not deterministic (check fn)
    // If clock warps too much in future, starts going back??
    tt.advance_clock_past_timestamp(now as i64 + 6011).await;
    // Best to read clock again
    let new_now = tt.bench.get_clock().await.unix_timestamp as u64;

    let withdraw_stream_ix = WithdrawStreamIx {
        ix: 1,
        amount: spl_token::ui_amount_to_amount(25.0, 8),
    };

    let withdraw_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &withdraw_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true),
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    tt.bench
        .process_transaction(&[withdraw_stream_ix_bytes], Some(&[&bob]))
        .await?;

    let metadata_data: TokenStreamData = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(
        metadata_data.withdrawn_amount,
        spl_token::ui_amount_to_amount(25.0, 8)
    );
    assert_eq!(metadata_data.last_withdrawn_at, new_now);

    // Try to topup, stream expired, shouldn't succeed
    let topup_ix = TopUpIx {
        ix: 4,
        amount: spl_token::ui_amount_to_amount(10.0, 8),
    }; // 4 => topup_stream
    let topupix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &topup_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    let transaction_error = tt.bench
        .process_transaction(&[topupix_bytes], Some(&[&alice]))
        .await;
    // Stream closed, no topup
    assert!(transaction_error.is_err());

    let cancel_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &cancel_ix.try_to_vec()?,
        vec![
            AccountMeta::new(some_other_kp.pubkey(), true), // RANDOM KEY
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    // Now stream should be cancelled, escrow closed
    tt.bench
        .process_transaction(&[cancel_ix_bytes], Some(&[&some_other_kp]))
        .await?;
    Ok(())
}

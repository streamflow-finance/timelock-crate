use std::str::FromStr;

use anyhow::Result;
use borsh::BorshSerialize;
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

use streamflow_timelock::state::{
    find_escrow_account, Contract, CreateParams, PROGRAM_VERSION, STRM_TREASURY,
};

mod fascilities;

use fascilities::*;

#[tokio::test]
async fn test_cancel_success() -> Result<()> {
    let strm_key = Pubkey::from_str(STRM_TREASURY).unwrap();
    let metadata_kp = Keypair::new();
    let alice = Account { lamports: sol_to_lamports(10.0), ..Account::default() };
    let bob = Account { lamports: sol_to_lamports(10.0), ..Account::default() };

    let mut tt = TimelockProgramTest::start_new(&[alice, bob], &strm_key).await;

    let alice = clone_keypair(&tt.accounts[0]);
    let bob = clone_keypair(&tt.accounts[1]);
    let partner = clone_keypair(&tt.accounts[2]);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());
    let strm_ass_token = get_associated_token_address(&strm_key, &strm_token_mint.pubkey());
    let partner_ass_token =
        get_associated_token_address(&partner.pubkey(), &strm_token_mint.pubkey());

    tt.bench.create_mint(&strm_token_mint, &tt.bench.payer.pubkey()).await;

    tt.bench.create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey()).await;

    tt.bench
        .mint_tokens(
            &strm_token_mint.pubkey(),
            &payer,
            &alice_ass_token,
            spl_token::ui_amount_to_amount(100000.0, 8),
        )
        .await;

    let alice_ass_account = tt.bench.get_account(&alice_ass_token).await.unwrap();
    let alice_token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(alice_token_data.amount, spl_token::ui_amount_to_amount(100000.0, 8));
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let escrow_tokens_pubkey =
        find_escrow_account(PROGRAM_VERSION, metadata_kp.pubkey().as_ref(), &tt.program_id).0;

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;
    let transfer_amount = 20;
    let amount_per_period = spl_token::ui_amount_to_amount(0.01, 8);
    let period = 1;

    let cancelable_by_sender = true;
    let cliff = now + 40;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff,
            cliff_amount: spl_token::ui_amount_to_amount(transfer_amount as f64 / 2.0, 8),
            cancelable_by_sender,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
            ..Default::default()
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
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(tt.fees_acc, false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let transaction = tt
        .bench
        .process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp]))
        .await;
    let is_err = transaction.is_err();
    assert!(!is_err);

    let periods_passed = 200;
    let _periods_after_cliff = now + periods_passed - cliff;
    tt.advance_clock_past_timestamp((now + periods_passed) as i64).await;

    let cancel_ix = CancelIx { ix: 2 };

    let cancel_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &cancel_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true), // sender
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    let transaction = tt.bench.process_transaction(&[cancel_ix_bytes], Some(&[&alice])).await;

    assert_eq!(transaction.is_err(), !cancelable_by_sender);

    let strm_expected_fee_total =
        (0.0025 * spl_token::ui_amount_to_amount(transfer_amount as f64, 8) as f64) as u64;
    let strm_expected_fee_withdrawn = 2957500;
    let recipient_expected_withdrawn = 1183000000;

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(metadata_data.ix.cancelable_by_sender, cancelable_by_sender);
    assert_eq!(metadata_data.streamflow_fee_percent, 0.25);
    assert_eq!(metadata_data.streamflow_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.partner_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.streamflow_fee_withdrawn, strm_expected_fee_withdrawn);
    assert_eq!(metadata_data.partner_fee_withdrawn, strm_expected_fee_withdrawn);
    assert_eq!(metadata_data.amount_withdrawn, recipient_expected_withdrawn);
    let bob_tokens = get_token_balance(&mut tt.bench.context.banks_client, bob_ass_token).await;
    assert_eq!(bob_tokens, recipient_expected_withdrawn);

    Ok(())
}

#[tokio::test]
async fn test_cancel_expired() -> Result<()> {
    let strm_key = Pubkey::from_str(STRM_TREASURY).unwrap();
    let metadata_kp = Keypair::new();
    let alice = Account { lamports: sol_to_lamports(10.0), ..Account::default() };
    let bob = Account { lamports: sol_to_lamports(10.0), ..Account::default() };

    let mut tt = TimelockProgramTest::start_new(&[alice, bob], &strm_key).await;

    let alice = clone_keypair(&tt.accounts[0]);
    let bob = clone_keypair(&tt.accounts[1]);
    let partner = clone_keypair(&tt.accounts[2]);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());
    let strm_ass_token = get_associated_token_address(&strm_key, &strm_token_mint.pubkey());
    let partner_ass_token =
        get_associated_token_address(&partner.pubkey(), &strm_token_mint.pubkey());

    tt.bench.create_mint(&strm_token_mint, &tt.bench.payer.pubkey()).await;

    tt.bench.create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey()).await;

    tt.bench
        .mint_tokens(
            &strm_token_mint.pubkey(),
            &payer,
            &alice_ass_token,
            spl_token::ui_amount_to_amount(100000.0, 8),
        )
        .await;

    let alice_ass_account = tt.bench.get_account(&alice_ass_token).await.unwrap();
    let alice_token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(alice_token_data.amount, spl_token::ui_amount_to_amount(100000.0, 8));
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let escrow_tokens_pubkey =
        find_escrow_account(PROGRAM_VERSION, metadata_kp.pubkey().as_ref(), &tt.program_id).0;

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;
    let transfer_amount = 20;
    let amount_per_period = spl_token::ui_amount_to_amount(1.0, 8);
    let period = 1;

    let cancelable_by_sender = false;
    let cliff = now + 40;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff,
            cliff_amount: spl_token::ui_amount_to_amount(transfer_amount as f64 / 2.0, 8),
            cancelable_by_sender,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
            ..Default::default()
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
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(tt.fees_acc, false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let transaction = tt
        .bench
        .process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp]))
        .await;
    let is_err = transaction.is_err();
    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert!(!is_err);

    let periods_passed = 200;
    tt.advance_clock_past_timestamp((now + periods_passed) as i64).await;
    assert!(now < metadata_data.end_time);
    assert!(!is_err);

    let cancel_ix = CancelIx { ix: 2 };

    let cancel_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &cancel_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true), // sender
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    let transaction = tt.bench.process_transaction(&[cancel_ix_bytes], Some(&[&alice])).await;
    let is_err = transaction.is_err();
    assert!(!is_err);

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(metadata_data.partner_fee_total, metadata_data.partner_fee_withdrawn);
    assert_eq!(metadata_data.streamflow_fee_total, metadata_data.streamflow_fee_withdrawn);
    assert_eq!(metadata_data.ix.net_amount_deposited, metadata_data.amount_withdrawn);
    let bob_tokens = get_token_balance(&mut tt.bench.context.banks_client, bob_ass_token).await;
    assert_eq!(bob_tokens, metadata_data.amount_withdrawn);

    Ok(())
}

#[tokio::test]
async fn test_not_cancelable_sender() -> Result<()> {
    let strm_key = Pubkey::from_str(STRM_TREASURY).unwrap();
    let metadata_kp = Keypair::new();
    let alice = Account { lamports: sol_to_lamports(10.0), ..Account::default() };
    let bob = Account { lamports: sol_to_lamports(10.0), ..Account::default() };

    let mut tt = TimelockProgramTest::start_new(&[alice, bob], &strm_key).await;

    let alice = clone_keypair(&tt.accounts[0]);
    let bob = clone_keypair(&tt.accounts[1]);
    let partner = clone_keypair(&tt.accounts[2]);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());
    let strm_ass_token = get_associated_token_address(&strm_key, &strm_token_mint.pubkey());
    let partner_ass_token =
        get_associated_token_address(&partner.pubkey(), &strm_token_mint.pubkey());

    tt.bench.create_mint(&strm_token_mint, &tt.bench.payer.pubkey()).await;

    tt.bench.create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey()).await;

    tt.bench
        .mint_tokens(
            &strm_token_mint.pubkey(),
            &payer,
            &alice_ass_token,
            spl_token::ui_amount_to_amount(100000.0, 8),
        )
        .await;

    let alice_ass_account = tt.bench.get_account(&alice_ass_token).await.unwrap();
    let alice_token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(alice_token_data.amount, spl_token::ui_amount_to_amount(100000.0, 8));
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let escrow_tokens_pubkey =
        find_escrow_account(PROGRAM_VERSION, metadata_kp.pubkey().as_ref(), &tt.program_id).0;

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;
    let transfer_amount = 20;
    let amount_per_period = spl_token::ui_amount_to_amount(0.01, 8);
    let period = 1;

    let cancelable_by_sender = false;
    let cliff = now + 40;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff,
            cliff_amount: spl_token::ui_amount_to_amount(transfer_amount as f64 / 2.0, 8),
            cancelable_by_sender,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
            ..Default::default()
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
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(tt.fees_acc, false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let transaction = tt
        .bench
        .process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp]))
        .await;
    let is_err = transaction.is_err();
    assert!(!is_err);

    let periods_passed = 200;
    let _periods_after_cliff = now + periods_passed - cliff;
    tt.advance_clock_past_timestamp((now + periods_passed) as i64).await;

    let cancel_ix = CancelIx { ix: 2 };

    let cancel_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &cancel_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true), // sender
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    let transaction = tt.bench.process_transaction(&[cancel_ix_bytes], Some(&[&alice])).await;
    assert_eq!(transaction.is_err(), !cancelable_by_sender);

    Ok(())
}

#[tokio::test]
async fn test_not_cancelable_recipient() -> Result<()> {
    let strm_key = Pubkey::from_str(STRM_TREASURY).unwrap();
    let metadata_kp = Keypair::new();
    let alice = Account { lamports: sol_to_lamports(10.0), ..Account::default() };
    let bob = Account { lamports: sol_to_lamports(10.0), ..Account::default() };

    let mut tt = TimelockProgramTest::start_new(&[alice, bob], &strm_key).await;

    let alice = clone_keypair(&tt.accounts[0]);
    let bob = clone_keypair(&tt.accounts[1]);
    let partner = clone_keypair(&tt.accounts[2]);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());
    let strm_ass_token = get_associated_token_address(&strm_key, &strm_token_mint.pubkey());
    let partner_ass_token =
        get_associated_token_address(&partner.pubkey(), &strm_token_mint.pubkey());

    tt.bench.create_mint(&strm_token_mint, &tt.bench.payer.pubkey()).await;

    tt.bench.create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey()).await;

    tt.bench
        .mint_tokens(
            &strm_token_mint.pubkey(),
            &payer,
            &alice_ass_token,
            spl_token::ui_amount_to_amount(100000.0, 8),
        )
        .await;

    let alice_ass_account = tt.bench.get_account(&alice_ass_token).await.unwrap();
    let alice_token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(alice_token_data.amount, spl_token::ui_amount_to_amount(100000.0, 8));
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let escrow_tokens_pubkey =
        find_escrow_account(PROGRAM_VERSION, metadata_kp.pubkey().as_ref(), &tt.program_id).0;

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;
    let transfer_amount = 20;
    let amount_per_period = spl_token::ui_amount_to_amount(0.01, 8);
    let period = 1;

    let cancelable_by_recipient = false;
    let cliff = now + 40;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff,
            cliff_amount: spl_token::ui_amount_to_amount(transfer_amount as f64 / 2.0, 8),
            cancelable_by_sender: false,
            cancelable_by_recipient,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
            ..Default::default()
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
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(tt.fees_acc, false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let transaction = tt
        .bench
        .process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp]))
        .await;
    let is_err = transaction.is_err();
    assert!(!is_err);

    let periods_passed = 200;
    let _periods_after_cliff = now + periods_passed - cliff;
    tt.advance_clock_past_timestamp((now + periods_passed) as i64).await;

    let cancel_ix = CancelIx { ix: 2 };

    let cancel_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &cancel_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true), // sender
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    let transaction = tt.bench.process_transaction(&[cancel_ix_bytes], Some(&[&bob])).await;
    assert_eq!(transaction.is_err(), !cancelable_by_recipient);

    Ok(())
}

#[tokio::test]
async fn test_cancel_expired_no_check() -> Result<()> {
    let strm_key = Pubkey::from_str(STRM_TREASURY).unwrap();
    let metadata_kp = Keypair::new();
    let alice = Account { lamports: sol_to_lamports(10.0), ..Account::default() };
    let bob = Account { lamports: sol_to_lamports(10.0), ..Account::default() };

    let mut tt = TimelockProgramTest::start_new(&[alice, bob], &strm_key).await;

    let alice = clone_keypair(&tt.accounts[0]);
    let bob = clone_keypair(&tt.accounts[1]);
    let partner = clone_keypair(&tt.accounts[2]);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());
    let strm_ass_token = get_associated_token_address(&strm_key, &strm_token_mint.pubkey());
    let partner_ass_token =
        get_associated_token_address(&partner.pubkey(), &strm_token_mint.pubkey());

    tt.bench.create_mint(&strm_token_mint, &tt.bench.payer.pubkey()).await;

    tt.bench.create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey()).await;

    tt.bench
        .mint_tokens(
            &strm_token_mint.pubkey(),
            &payer,
            &alice_ass_token,
            spl_token::ui_amount_to_amount(100000.0, 8),
        )
        .await;

    let alice_ass_account = tt.bench.get_account(&alice_ass_token).await.unwrap();
    let alice_token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(alice_token_data.amount, spl_token::ui_amount_to_amount(100000.0, 8));
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let escrow_tokens_pubkey =
        find_escrow_account(PROGRAM_VERSION, metadata_kp.pubkey().as_ref(), &tt.program_id).0;

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;
    let transfer_amount = 20;
    let amount_per_period = spl_token::ui_amount_to_amount(1.0, 8);
    let period = 1;

    let cancelable_by_sender = false;
    let cliff = now + 40;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff,
            cliff_amount: spl_token::ui_amount_to_amount(transfer_amount as f64 / 2.0, 8),
            cancelable_by_sender,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
            ..Default::default()
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
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(tt.fees_acc, false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let transaction = tt
        .bench
        .process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp]))
        .await;
    let is_err = transaction.is_err();
    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert!(!is_err);

    let periods_passed = 200;
    tt.advance_clock_past_timestamp((now + periods_passed) as i64).await;
    assert!(now < metadata_data.end_time);
    assert!(!is_err);

    let cancel_ix = CancelIx { ix: 2 };

    let cancel_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &cancel_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true), // sender
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    let transaction = tt.bench.process_transaction(&[cancel_ix_bytes], Some(&[&alice])).await;
    let is_err = transaction.is_err();
    assert!(!is_err);
    Ok(())
}

#[tokio::test]
async fn test_cancel_not_signer() -> Result<()> {
    let strm_key = Pubkey::from_str(STRM_TREASURY).unwrap();
    let metadata_kp = Keypair::new();
    let alice = Account { lamports: sol_to_lamports(10.0), ..Account::default() };
    let bob = Account { lamports: sol_to_lamports(10.0), ..Account::default() };

    let mut tt = TimelockProgramTest::start_new(&[alice, bob], &strm_key).await;

    let alice = clone_keypair(&tt.accounts[0]);
    let bob = clone_keypair(&tt.accounts[1]);
    let partner = clone_keypair(&tt.accounts[2]);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());
    let strm_ass_token = get_associated_token_address(&strm_key, &strm_token_mint.pubkey());
    let partner_ass_token =
        get_associated_token_address(&partner.pubkey(), &strm_token_mint.pubkey());

    tt.bench.create_mint(&strm_token_mint, &tt.bench.payer.pubkey()).await;

    tt.bench.create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey()).await;

    tt.bench
        .mint_tokens(
            &strm_token_mint.pubkey(),
            &payer,
            &alice_ass_token,
            spl_token::ui_amount_to_amount(100000.0, 8),
        )
        .await;

    let alice_ass_account = tt.bench.get_account(&alice_ass_token).await.unwrap();
    let alice_token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(alice_token_data.amount, spl_token::ui_amount_to_amount(100000.0, 8));
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let escrow_tokens_pubkey =
        find_escrow_account(PROGRAM_VERSION, metadata_kp.pubkey().as_ref(), &tt.program_id).0;

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;
    let transfer_amount = 20;
    let amount_per_period = spl_token::ui_amount_to_amount(0.01, 8);
    let period = 1;

    let cancelable_by_sender = true;
    let cliff = now + 40;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff,
            cliff_amount: spl_token::ui_amount_to_amount(transfer_amount as f64 / 2.0, 8),
            cancelable_by_sender,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
            ..Default::default()
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
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(tt.fees_acc, false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let transaction = tt
        .bench
        .process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp]))
        .await;
    let is_err = transaction.is_err();
    assert!(!is_err);

    let periods_passed = 200;
    let _periods_after_cliff = now + periods_passed - cliff;
    tt.advance_clock_past_timestamp((now + periods_passed) as i64).await;

    let cancel_ix = CancelIx { ix: 2 };

    let cancel_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &cancel_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), false), // sender
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(bob.pubkey(), false),
            AccountMeta::new(bob_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    let transaction = tt.bench.process_transaction(&[cancel_ix_bytes], Some(&[])).await;

    assert!(transaction.is_err());

    Ok(())
}

#[tokio::test]
async fn test_cancel_self_stream_success() -> Result<()> {
    let strm_key = Pubkey::from_str(STRM_TREASURY).unwrap();
    let metadata_kp = Keypair::new();
    let alice = Account { lamports: sol_to_lamports(10.0), ..Account::default() };
    let bob = Account { lamports: sol_to_lamports(10.0), ..Account::default() };

    let mut tt = TimelockProgramTest::start_new(&[alice, bob], &strm_key).await;

    let alice = clone_keypair(&tt.accounts[0]);
    let bob = clone_keypair(&tt.accounts[1]);
    let partner = clone_keypair(&tt.accounts[2]);
    let payer = clone_keypair(&tt.bench.payer);

    let strm_token_mint = Keypair::new();
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let _bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());
    let strm_ass_token = get_associated_token_address(&strm_key, &strm_token_mint.pubkey());
    let partner_ass_token =
        get_associated_token_address(&partner.pubkey(), &strm_token_mint.pubkey());

    tt.bench.create_mint(&strm_token_mint, &tt.bench.payer.pubkey()).await;

    tt.bench.create_associated_token_account(&strm_token_mint.pubkey(), &alice.pubkey()).await;

    tt.bench
        .mint_tokens(
            &strm_token_mint.pubkey(),
            &payer,
            &alice_ass_token,
            spl_token::ui_amount_to_amount(100000.0, 8),
        )
        .await;

    let alice_ass_account = tt.bench.get_account(&alice_ass_token).await.unwrap();
    let alice_token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(alice_token_data.amount, spl_token::ui_amount_to_amount(100000.0, 8));
    assert_eq!(alice_token_data.mint, strm_token_mint.pubkey());
    assert_eq!(alice_token_data.owner, alice.pubkey());

    let escrow_tokens_pubkey =
        find_escrow_account(PROGRAM_VERSION, metadata_kp.pubkey().as_ref(), &tt.program_id).0;

    let clock = tt.bench.get_clock().await;
    let now = clock.unix_timestamp as u64;
    let transfer_amount = 20;
    let amount_per_period = spl_token::ui_amount_to_amount(0.01, 8);
    let period = 1;

    let cancelable_by_sender = true;
    let cliff = now + 40;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff,
            cliff_amount: spl_token::ui_amount_to_amount(transfer_amount as f64 / 2.0, 8),
            cancelable_by_sender,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
            ..Default::default()
        },
    };

    let create_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &create_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), true),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(tt.fees_acc, false),
            AccountMeta::new_readonly(rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let transaction = tt
        .bench
        .process_transaction(&[create_stream_ix_bytes], Some(&[&alice, &metadata_kp]))
        .await;
    let is_err = transaction.is_err();
    assert!(!is_err);

    let periods_passed = 200;
    let _periods_after_cliff = now + periods_passed - cliff;
    tt.advance_clock_past_timestamp((now + periods_passed) as i64).await;

    let cancel_ix = CancelIx { ix: 2 };

    let cancel_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &cancel_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true), // sender
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(alice.pubkey(), false),
            AccountMeta::new(alice_ass_token, false),
            AccountMeta::new(metadata_kp.pubkey(), false),
            AccountMeta::new(escrow_tokens_pubkey, false),
            AccountMeta::new(strm_key, false),
            AccountMeta::new(strm_ass_token, false),
            AccountMeta::new(partner.pubkey(), false),
            AccountMeta::new(partner_ass_token, false),
            AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
    );

    let transaction = tt.bench.process_transaction(&[cancel_ix_bytes], Some(&[&alice])).await;

    assert_eq!(transaction.is_err(), false);

    let strm_expected_fee_total =
        (0.0025 * spl_token::ui_amount_to_amount(transfer_amount as f64, 8) as f64) as u64;
    let strm_expected_fee_withdrawn = 2957500;
    let recipient_expected_withdrawn = 1183000000;

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(metadata_data.ix.cancelable_by_sender, cancelable_by_sender);
    assert_eq!(metadata_data.streamflow_fee_percent, 0.25);
    assert_eq!(metadata_data.streamflow_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.partner_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.streamflow_fee_withdrawn, strm_expected_fee_withdrawn);
    assert_eq!(metadata_data.partner_fee_withdrawn, strm_expected_fee_withdrawn);
    assert_eq!(metadata_data.amount_withdrawn, recipient_expected_withdrawn);

    Ok(())
}

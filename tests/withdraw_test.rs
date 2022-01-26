use std::str::FromStr;

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

use streamflow_timelock::{
    error::SfError,
    state::{find_escrow_account, Contract, CreateParams, PROGRAM_VERSION, STRM_TREASURY},
};

mod fascilities;

use fascilities::*;

#[tokio::test]
async fn test_withdraw_stream_success() -> Result<()> {
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
    let amount_per_period = spl_token::ui_amount_to_amount(0.1, 8);
    let period = 1;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
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

    let periods_passed = 50;
    tt.advance_clock_past_timestamp(now as i64 + periods_passed).await;

    let withdraw_amount = spl_token::ui_amount_to_amount(1.0, 8);
    let withdraw_stream_ix = WithdrawStreamIx { ix: 1, amount: withdraw_amount };

    let withdraw_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &withdraw_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true),
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

    let transaction =
        tt.bench.process_transaction(&[withdraw_stream_ix_bytes], Some(&[&bob])).await;

    let is_err = transaction.is_err();
    assert!(!is_err);

    let strm_expected_fee_total =
        (0.0025 * spl_token::ui_amount_to_amount(transfer_amount as f64, 8) as f64) as u64;
    let strm_expected_fee_withdrawn = 1575000;

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(metadata_data.streamflow_fee_percent, 0.25);
    assert_eq!(metadata_data.streamflow_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.partner_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.streamflow_fee_withdrawn, strm_expected_fee_withdrawn);
    assert_eq!(metadata_data.partner_fee_withdrawn, strm_expected_fee_withdrawn);

    // check if external deposits were identified
    assert_eq!(
        metadata_data.ix.net_amount_deposited,
        spl_token::ui_amount_to_amount(transfer_amount as f64, 8)
    );
    let bob_tokens = get_token_balance(&mut tt.bench.context.banks_client, bob_ass_token).await;
    assert_eq!(bob_tokens, metadata_data.amount_withdrawn);

    Ok(())
}

#[tokio::test]
async fn test_withdraw_small_amount() -> Result<()> {
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
    let amount_per_period = spl_token::ui_amount_to_amount(0.1, 8);
    let period = 1;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
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

    let periods_passed = 50;
    tt.advance_clock_past_timestamp(now as i64 + periods_passed).await;

    let withdraw_amount = spl_token::ui_amount_to_amount(0.000001, 8);
    let withdraw_stream_ix = WithdrawStreamIx { ix: 1, amount: withdraw_amount };

    let withdraw_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &withdraw_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true),
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

    let transaction =
        tt.bench.process_transaction(&[withdraw_stream_ix_bytes], Some(&[&bob])).await;

    let is_err = transaction.is_err();
    assert!(!is_err);

    let strm_expected_fee_total =
        (0.0025 * spl_token::ui_amount_to_amount(transfer_amount as f64, 8) as f64) as u64;
    let strm_expected_fee_withdrawn = 1575000;

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(metadata_data.streamflow_fee_percent, 0.25);
    assert_eq!(metadata_data.streamflow_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.partner_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.streamflow_fee_withdrawn, strm_expected_fee_withdrawn);
    assert_eq!(metadata_data.partner_fee_withdrawn, strm_expected_fee_withdrawn);

    // check if external deposits were identified
    assert_eq!(
        metadata_data.ix.net_amount_deposited,
        spl_token::ui_amount_to_amount(transfer_amount as f64, 8)
    );
    let bob_tokens = get_token_balance(&mut tt.bench.context.banks_client, bob_ass_token).await;
    assert_eq!(bob_tokens, metadata_data.amount_withdrawn);

    Ok(())
}

#[tokio::test]
async fn test_withdraw_external_deposit() -> Result<()> {
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
    let amount_per_period = spl_token::ui_amount_to_amount(0.1, 8);
    let period = 1;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: true,
            stream_name: TEST_STREAM_NAME,
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

    let periods_passed = 50;
    tt.advance_clock_past_timestamp(now as i64 + periods_passed).await;

    let external_deposit_amount = 10;
    tt.bench
        .mint_tokens(
            &strm_token_mint.pubkey(),
            &payer,
            &escrow_tokens_pubkey,
            spl_token::ui_amount_to_amount(external_deposit_amount as f64, 8),
        )
        .await;

    let withdraw_amount = spl_token::ui_amount_to_amount(1.0, 8);
    let withdraw_stream_ix = WithdrawStreamIx { ix: 1, amount: withdraw_amount };

    let withdraw_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &withdraw_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true),
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

    let transaction =
        tt.bench.process_transaction(&[withdraw_stream_ix_bytes], Some(&[&bob])).await;

    let is_err = transaction.is_err();
    assert!(!is_err);

    let strm_expected_fee_total =
        (0.0025 * spl_token::ui_amount_to_amount(transfer_amount as f64, 8) as f64) as u64;
    let strm_external_deposit_fee =
        (0.0025 * spl_token::ui_amount_to_amount(external_deposit_amount as f64, 8) as f64) as u64;
    let strm_expected_fee_withdrawn = 1575000;

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(metadata_data.streamflow_fee_percent, 0.25);
    assert_eq!(
        metadata_data.streamflow_fee_total,
        strm_expected_fee_total + strm_external_deposit_fee
    );
    assert_eq!(
        metadata_data.partner_fee_total,
        strm_expected_fee_total + strm_external_deposit_fee
    );
    assert_eq!(metadata_data.streamflow_fee_withdrawn, strm_expected_fee_withdrawn);
    assert_eq!(metadata_data.partner_fee_withdrawn, strm_expected_fee_withdrawn);

    // check if external deposits were identified
    assert_eq!(
        metadata_data.ix.net_amount_deposited,
        spl_token::ui_amount_to_amount(transfer_amount as f64, 8) +
            spl_token::ui_amount_to_amount(external_deposit_amount as f64, 8) -
            2 * strm_external_deposit_fee
    );
    let bob_tokens = get_token_balance(&mut tt.bench.context.banks_client, bob_ass_token).await;
    assert_eq!(bob_tokens, metadata_data.amount_withdrawn);

    Ok(())
}

#[tokio::test]
async fn test_withdraw_stream_cliff() -> Result<()> {
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
    let amount_per_period = spl_token::ui_amount_to_amount(0.1, 8);
    let period = 1;
    let cliff_amount = spl_token::ui_amount_to_amount(transfer_amount as f64 / 2.0, 8);
    let amount = spl_token::ui_amount_to_amount(transfer_amount as f64, 8);
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: amount,
            period,
            amount_per_period,
            cliff: now + 50,
            cliff_amount,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
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

    let periods_passed = 60;
    tt.advance_clock_past_timestamp(now as i64 + periods_passed).await;

    let withdraw_amount = spl_token::ui_amount_to_amount(1.0, 8);
    let withdraw_stream_ix = WithdrawStreamIx { ix: 1, amount: withdraw_amount };

    let withdraw_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &withdraw_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true),
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

    let transaction =
        tt.bench.process_transaction(&[withdraw_stream_ix_bytes], Some(&[&bob])).await;

    let is_err = transaction.is_err();
    assert!(!is_err);

    let strm_expected_fee_total = (0.0025 * amount as f64) as u64;
    let strm_expected_fee_withdrawn = 2950000;

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(metadata_data.streamflow_fee_percent, 0.25);
    assert_eq!(metadata_data.streamflow_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.partner_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.streamflow_fee_withdrawn, strm_expected_fee_withdrawn);
    assert_eq!(metadata_data.partner_fee_withdrawn, strm_expected_fee_withdrawn);

    // check if external deposits were identified
    assert_eq!(metadata_data.ix.net_amount_deposited, amount);
    let bob_tokens = get_token_balance(&mut tt.bench.context.banks_client, bob_ass_token).await;
    assert_eq!(bob_tokens, metadata_data.amount_withdrawn);

    Ok(())
}

#[tokio::test]
async fn test_withdraw_expired_stream() -> Result<()> {
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
    let amount_per_period = spl_token::ui_amount_to_amount(0.1, 8);
    let period = 1;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
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

    let periods_passed = 500;
    tt.advance_clock_past_timestamp(now as i64 + periods_passed).await;

    let withdraw_amount = spl_token::ui_amount_to_amount(1.0, 8);
    let withdraw_stream_ix = WithdrawStreamIx { ix: 1, amount: withdraw_amount };

    let withdraw_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &withdraw_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), true),
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

    let transaction =
        tt.bench.process_transaction(&[withdraw_stream_ix_bytes], Some(&[&bob])).await;

    let is_err = transaction.is_err();
    assert!(!is_err);

    let strm_expected_fee_total =
        (0.0025 * spl_token::ui_amount_to_amount(transfer_amount as f64, 8) as f64) as u64;

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(metadata_data.streamflow_fee_percent, 0.25);
    assert_eq!(metadata_data.streamflow_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.partner_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.streamflow_fee_withdrawn, strm_expected_fee_total);
    assert_eq!(metadata_data.partner_fee_withdrawn, strm_expected_fee_total);
    assert_eq!(metadata_data.amount_withdrawn, metadata_data.ix.net_amount_deposited);

    // check if external deposits were identified
    assert_eq!(
        metadata_data.ix.net_amount_deposited,
        spl_token::ui_amount_to_amount(transfer_amount as f64, 8)
    );
    let bob_tokens = get_token_balance(&mut tt.bench.context.banks_client, bob_ass_token).await;
    assert_eq!(bob_tokens, metadata_data.amount_withdrawn);

    Ok(())
}

#[tokio::test]
async fn test_withdraw_not_signer() -> Result<()> {
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
    let amount_per_period = spl_token::ui_amount_to_amount(0.1, 8);
    let period = 1;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
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

    let periods_passed = 50;
    tt.advance_clock_past_timestamp(now as i64 + periods_passed).await;

    let withdraw_amount = spl_token::ui_amount_to_amount(1.0, 8);
    let withdraw_stream_ix = WithdrawStreamIx { ix: 1, amount: withdraw_amount };

    let withdraw_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &withdraw_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(bob.pubkey(), false),
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

    let transaction = tt.bench.process_transaction(&[withdraw_stream_ix_bytes], Some(&[])).await;

    let is_err = transaction.is_err();
    assert!(is_err);

    Ok(())
}

#[tokio::test]
async fn test_withdraw_stream_self_stream() -> Result<()> {
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
    let amount_per_period = spl_token::ui_amount_to_amount(0.1, 8);
    let period = 1;
    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: CreateParams {
            start_time: now + 5,
            net_amount_deposited: spl_token::ui_amount_to_amount(transfer_amount as f64, 8),
            period,
            amount_per_period,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            automatic_withdrawal: false,
            transferable_by_sender: false,
            transferable_by_recipient: false,
            can_topup: false,
            stream_name: TEST_STREAM_NAME,
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

    let periods_passed = 50;
    tt.advance_clock_past_timestamp(now as i64 + periods_passed).await;

    let withdraw_amount = spl_token::ui_amount_to_amount(1.0, 8);
    let withdraw_stream_ix = WithdrawStreamIx { ix: 1, amount: withdraw_amount };

    let withdraw_stream_ix_bytes = Instruction::new_with_bytes(
        tt.program_id,
        &withdraw_stream_ix.try_to_vec()?,
        vec![
            AccountMeta::new(alice.pubkey(), true),
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

    let transaction =
        tt.bench.process_transaction(&[withdraw_stream_ix_bytes], Some(&[&alice])).await;

    let is_err = transaction.is_err();
    assert!(!is_err);

    let strm_expected_fee_total =
        (0.0025 * spl_token::ui_amount_to_amount(transfer_amount as f64, 8) as f64) as u64;
    let strm_expected_fee_withdrawn = 1575000;

    let metadata_data: Contract = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(metadata_data.streamflow_fee_percent, 0.25);
    assert_eq!(metadata_data.streamflow_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.partner_fee_total, strm_expected_fee_total);
    assert_eq!(metadata_data.streamflow_fee_withdrawn, strm_expected_fee_withdrawn);
    assert_eq!(metadata_data.partner_fee_withdrawn, strm_expected_fee_withdrawn);

    // check if external deposits were identified
    assert_eq!(
        metadata_data.ix.net_amount_deposited,
        spl_token::ui_amount_to_amount(transfer_amount as f64, 8)
    );

    Ok(())
}

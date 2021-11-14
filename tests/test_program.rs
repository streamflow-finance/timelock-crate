use anyhow::Result;
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::system_instruction;
use solana_program_test::{processor, tokio, ProgramTest};
use solana_sdk::{
    account::Account,
    borsh as solana_borsh,
    instruction::{AccountMeta, Instruction},
    native_token::sol_to_lamports,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::Signer,
    signer::keypair::Keypair,
    system_program,
    sysvar::rent,
    transaction::Transaction,
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use std::time::SystemTime;

use streamflow_timelock::entrypoint::process_instruction;
use streamflow_timelock::state::{StreamInstruction, TokenStreamData};

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
struct CancelStreamIx {
    ix: u8,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
struct TransferRecipientIx {
    ix: u8,
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
    runtime.add_account(
        bob.pubkey(),
        Account {
            lamports: sol_to_lamports(1.0),
            ..Account::default()
        },
    );

    let mut ctx = runtime.start_with_context().await;

    // Create a new SPL token
    let rent = ctx.banks_client.get_rent().await?;
    let strm_token_mint = Keypair::new();

    // Let's also find the associated token accounts for Alice & Bob
    let alice_ass_token = get_associated_token_address(&alice.pubkey(), &strm_token_mint.pubkey());
    let bob_ass_token = get_associated_token_address(&bob.pubkey(), &strm_token_mint.pubkey());

    // Build a transaction to initialize our mint.
    let mut tx = Transaction::new_with_payer(
        &[
            system_instruction::create_account(
                &ctx.payer.pubkey(),
                &strm_token_mint.pubkey(),
                rent.minimum_balance(spl_token::state::Mint::LEN),
                spl_token::state::Mint::LEN as u64,
                &spl_token::id(),
            ),
            spl_token::instruction::initialize_mint(
                &spl_token::id(),
                &strm_token_mint.pubkey(),
                &ctx.payer.pubkey(),
                None,
                8,
            )?,
        ],
        Some(&ctx.payer.pubkey()),
    );
    tx.sign(&[&ctx.payer, &strm_token_mint], ctx.last_blockhash);
    ctx.banks_client.process_transaction(tx).await?;

    // Once that is done, let's mint some to Alice.
    let mut tx = Transaction::new_with_payer(
        &[
            create_associated_token_account(
                &ctx.payer.pubkey(),
                &alice.pubkey(),
                &strm_token_mint.pubkey(),
            ),
            spl_token::instruction::mint_to(
                &spl_token::id(),
                &strm_token_mint.pubkey(),
                &alice_ass_token,
                &ctx.payer.pubkey(),
                &[],
                spl_token::ui_amount_to_amount(100.0, 8),
            )?,
        ],
        Some(&ctx.payer.pubkey()),
    );
    tx.sign(&[&ctx.payer], ctx.last_blockhash);
    ctx.banks_client.process_transaction(tx).await?;

    let alice_ass_account = ctx.banks_client.get_account(alice_ass_token).await?;
    let alice_ass_account = alice_ass_account.unwrap();
    let token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(token_data.mint, strm_token_mint.pubkey());
    assert_eq!(token_data.owner, alice.pubkey());
    assert_eq!(token_data.amount, spl_token::ui_amount_to_amount(100.0, 8));

    // Let's try to initialize a stream now.
    let metadata_acc = Keypair::new();
    let (escrow_tokens, _) =
        Pubkey::find_program_address(&[metadata_acc.pubkey().as_ref()], &program_id);

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 10;

    let stream_total_amount = 20.0;
    let stream_ix = StreamInstruction {
        start_time: now,
        end_time: now + 600,
        deposited_amount: 666,
        total_amount: spl_token::ui_amount_to_amount(stream_total_amount, 8),
        period: 1,
        cliff: now,
        cliff_amount: 0,
        cancelable_by_sender: false,
        cancelable_by_recipient: false,
        withdrawal_public: false,
        transferable: false,
        stream_name: "TheTestoooooooor".to_string(),
    };

    let create_stream_ix = CreateStreamIx {
        ix: 0,
        metadata: stream_ix,
    };

    let mut tx = Transaction::new_with_payer(
        &[Instruction::new_with_bytes(
            program_id,
            &create_stream_ix.try_to_vec()?,
            vec![
                AccountMeta::new(alice.pubkey(), true),
                AccountMeta::new(alice_ass_token, false),
                AccountMeta::new(bob.pubkey(), false),
                AccountMeta::new(bob_ass_token, false),
                AccountMeta::new(metadata_acc.pubkey(), true),
                AccountMeta::new(escrow_tokens, false),
                AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
                AccountMeta::new_readonly(rent::id(), false),
                AccountMeta::new_readonly(spl_token::id(), false),
                AccountMeta::new_readonly(spl_associated_token_account::id(), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
        )],
        Some(&alice.pubkey()),
    );
    tx.sign(&[&alice, &metadata_acc], ctx.last_blockhash);
    ctx.banks_client.process_transaction(tx).await?;
    let alice_ass_account = ctx.banks_client.get_account(alice_ass_token).await?;
    let alice_ass_account = alice_ass_account.unwrap();
    let token_data = spl_token::state::Account::unpack_from_slice(&alice_ass_account.data)?;
    assert_eq!(
        token_data.amount,
        spl_token::ui_amount_to_amount(100.0 - stream_total_amount, 8)
    );

    let metadata_account = ctx.banks_client.get_account(metadata_acc.pubkey()).await?;
    let metadata_account = metadata_account.unwrap();
    // This thing is nasty lol
    let metadata_data: TokenStreamData =
        solana_borsh::try_from_slice_unchecked(&metadata_account.data)?;

    println!("{:#?}", metadata_data);

    assert_eq!(metadata_account.owner, program_id);
    assert_eq!(metadata_data.magic, 0);
    assert_eq!(metadata_data.withdrawn_amount, 0);
    assert_eq!(metadata_data.canceled_at, 0);
    assert_eq!(metadata_data.cancellable_at, now + 600);
    assert_eq!(metadata_data.last_withdrawn_at, 0);
    assert_eq!(metadata_data.sender, alice.pubkey());
    assert_eq!(metadata_data.sender_tokens, alice_ass_token);
    assert_eq!(metadata_data.recipient, bob.pubkey());
    assert_eq!(metadata_data.recipient_tokens, bob_ass_token);
    assert_eq!(metadata_data.mint, strm_token_mint.pubkey());
    assert_eq!(metadata_data.escrow_tokens, escrow_tokens);
    assert_eq!(metadata_data.ix.start_time, now);
    assert_eq!(metadata_data.ix.end_time, now + 600);
    assert_eq!(
        metadata_data.ix.deposited_amount,
        spl_token::ui_amount_to_amount(stream_total_amount, 8)
    );
    assert_eq!(
        metadata_data.ix.total_amount,
        spl_token::ui_amount_to_amount(stream_total_amount, 8)
    );
    assert_eq!(metadata_data.ix.stream_name, "TheTestoooooooor".to_string());

    // Let's warp ahead and try withdrawing a piece of the stream.
    let expected_slot = 100_000;
    ctx.warp_to_slot(expected_slot)?;

    let withdraw_stream_ix = WithdrawStreamIx { ix: 1, amount: 0 };

    let mut tx = Transaction::new_with_payer(
        &[Instruction::new_with_bytes(
            program_id,
            &withdraw_stream_ix.try_to_vec()?,
            vec![
                AccountMeta::new(bob.pubkey(), true),
                AccountMeta::new(alice.pubkey(), false),
                AccountMeta::new(bob.pubkey(), false),
                AccountMeta::new(bob_ass_token, false),
                AccountMeta::new(metadata_acc.pubkey(), false),
                AccountMeta::new(escrow_tokens, false),
                AccountMeta::new_readonly(strm_token_mint.pubkey(), false),
                AccountMeta::new_readonly(spl_token::id(), false),
            ],
        )],
        Some(&bob.pubkey()),
    );
    tx.sign(&[&bob], ctx.last_blockhash);
    ctx.banks_client.process_transaction(tx).await?;

    let metadata_account = ctx.banks_client.get_account(metadata_acc.pubkey()).await?;
    let metadata_account = metadata_account.unwrap();
    // This thing is nasty lol
    let metadata_data: TokenStreamData =
        solana_borsh::try_from_slice_unchecked(&metadata_account.data)?;
    println!("{:#?}", metadata_data);

    let bob_ass_account = ctx.banks_client.get_account(bob_ass_token).await?;
    let bob_ass_account = bob_ass_account.unwrap();
    let bob_ass_data = spl_token::state::Account::unpack_from_slice(&bob_ass_account.data)?;

    assert_eq!(bob_ass_data.owner, bob.pubkey());
    assert_eq!(bob_ass_data.mint, strm_token_mint.pubkey());
    assert_eq!(bob_ass_data.amount, metadata_data.withdrawn_amount);

    Ok(())
}

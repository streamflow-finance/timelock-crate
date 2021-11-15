use anyhow::Result;
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::system_instruction;
use solana_program_test::{processor, tokio};
use solana_sdk::{
    account::Account,
    clock::UnixTimestamp,
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
use test_sdk::{tools::clone_keypair, ProgramTestBench, TestBenchProgram};

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
            deposited_amount: 0,
            total_amount: spl_token::ui_amount_to_amount(20.0, 8),
            period: 1,
            cliff: 0,
            cliff_amount: 0,
            cancelable_by_sender: false,
            cancelable_by_recipient: false,
            withdrawal_public: false,
            transferable: false,
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
    assert_eq!(metadata_data.magic, 0);
    assert_eq!(metadata_data.withdrawn_amount, 0);
    assert_eq!(metadata_data.canceled_at, 0);
    assert_eq!(metadata_data.cancellable_at, now + 605);
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

    let metadata_acc = tt.bench.get_account(&metadata_kp.pubkey()).await.unwrap();
    let metadata_data: TokenStreamData = tt.bench.get_borsh_account(&metadata_kp.pubkey()).await;
    assert_eq!(metadata_data.withdrawn_amount, 1180000000);

    println!("{:#?}", metadata_data);

    Ok(())
}

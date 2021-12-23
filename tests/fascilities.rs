use borsh::{BorshDeserialize, BorshSerialize};
use solana_program_test::processor;
use solana_sdk::{
    account::Account, clock::UnixTimestamp, native_token::sol_to_lamports, pubkey::Pubkey,
    signature::Signer, signer::keypair::Keypair,
};

use partner_oracle::fees::{Partner, Partners};
use solana_program_test::ProgramTest;

use streamflow_timelock::{entrypoint::process_instruction, state::StreamInstruction};
use test_sdk::ProgramTestBench;
use test_sdk::tools::clone_keypair;

#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct CreateStreamIx {
    pub ix: u8,
    pub metadata: StreamInstruction,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct WithdrawStreamIx {
    pub ix: u8,
    pub amount: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct TopUpIx {
    pub ix: u8,
    pub amount: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct CancelIx {
    pub ix: u8,
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct TransferIx {
    pub ix: u8,
}

pub struct TimelockProgramTest {
    pub bench: ProgramTestBench,
    pub program_id: Pubkey,
    pub accounts: Vec<Keypair>,
    pub fees_acc: Pubkey
    // pub accounts: TestAccounts,
}

/*
pub TestAccounts {
    alice: Keypair,
    bob: Keypair,
    ...
}
*/

impl TimelockProgramTest {
    pub async fn start_new(accounts: &[Account], strm_acc: &Pubkey) -> Self {
        let mut program_test = ProgramTest::default();

        let program_id = Keypair::new().pubkey();

        let mut accounts_kp = vec![];

        program_test.add_program(
            "streamflow_timelock",
            program_id,
            processor!(process_instruction),
        );

        program_test.add_program(
            "partner-oracle",
            partner_oracle::id(),
            processor!(partner_oracle::entrypoint::process_instruction),
        );

        program_test.add_account(
            *strm_acc,
            Account { lamports: sol_to_lamports(1.0), ..Account::default() },
        );

        for acc in accounts {
            let kp = Keypair::new();
            program_test.add_account(kp.pubkey(), acc.clone());
            accounts_kp.push(kp);
        }

        // Oracle & fees stuff
        let fees_acc_pubkey = Pubkey::find_program_address(
            &[&partner_oracle::FEES_METADATA_SEED],
            &partner_oracle::id(),
        )
        .0;

        let some_partner_kp =  Keypair::new();
        accounts_kp.push(clone_keypair(&some_partner_kp));
        let another_partner_kp = Keypair::new();
        accounts_kp.push(clone_keypair(&another_partner_kp));
        let strm_partner = Partner { pubkey: *strm_acc, partner_fee: 0.0, strm_fee: 0.25 };
        let some_partner =
            Partner { pubkey: some_partner_kp.pubkey(), partner_fee: 0.25, strm_fee: 0.25 };
        let another_partner =
            Partner { pubkey: another_partner_kp.pubkey(), partner_fee: 0.25, strm_fee: 0.25 };

        let partners = Partners(vec![strm_partner, some_partner, another_partner]);
        let partner_data_bytes = partners.try_to_vec().unwrap();

        program_test.add_account(
            fees_acc_pubkey,
            Account {
                lamports: sol_to_lamports(10.0),
                data: partner_data_bytes,
                owner: partner_oracle::id(),
                executable: false,
                rent_epoch: 1000000,
            },
        );

        let bench = ProgramTestBench::start_new(program_test).await;

        Self { bench, program_id, accounts: accounts_kp, fees_acc: fees_acc_pubkey }
    }

    pub async fn advance_clock_past_timestamp(&mut self, unix_timestamp: UnixTimestamp) {
        let mut clock = self.bench.get_clock().await;
        let mut n = 1;

        while clock.unix_timestamp <= unix_timestamp {
            // Since the exact time is not deterministic, keep wrapping by
            // arbitrary 400 slots until we pass the requested timestamp.
            self.bench.context.warp_to_slot(clock.slot + n * 400).unwrap();

            n += 1;
            clock = self.bench.get_clock().await;
        }
    }
}

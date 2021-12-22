use borsh::{BorshDeserialize, BorshSerialize};
use solana_program_test::processor;
use solana_sdk::{
    clock::UnixTimestamp,
    pubkey::Pubkey,
    signature::Signer,
    signer::keypair::Keypair,
    account::Account,
};

use test_sdk::{ProgramTestBench, TestBenchProgram};

use streamflow_timelock::entrypoint::process_instruction;
use streamflow_timelock::state::StreamInstruction;

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
}

impl TimelockProgramTest {
    pub async fn start_new(accounts: &[Account]) -> Self {
        let program_id = Keypair::new().pubkey();

        let program = TestBenchProgram {
            program_name: "streamflow_timelock",
            program_id,
            process_instruction: processor!(process_instruction),
        };

        let bench = ProgramTestBench::start_new(&[program], accounts).await;

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
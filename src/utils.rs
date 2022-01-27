use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    program_pack::Pack, pubkey::Pubkey,
};

use crate::try_math::*;

use crate::{error::SfError, state::CreateParams};

/// Do a sanity check with given Unix timestamps.
pub fn duration_sanity(now: u64, start: u64, cliff: u64) -> ProgramResult {
    let cliff_cond = if cliff == 0 { true } else { start <= cliff };

    if now < start && cliff_cond {
        return Ok(())
    }

    Err(SfError::InvalidTimestamps.into())
}

/// Unpack token account from `account_info`
pub fn unpack_token_account(
    account_info: &AccountInfo,
) -> Result<spl_token::state::Account, ProgramError> {
    if account_info.owner != &spl_token::id() {
        return Err(ProgramError::InvalidAccountData)
    }

    spl_token::state::Account::unpack(&account_info.data.borrow())
}

/// Unpack mint account from `account_info`
pub fn unpack_mint_account(
    account_info: &AccountInfo,
) -> Result<spl_token::state::Mint, ProgramError> {
    spl_token::state::Mint::unpack(&account_info.data.borrow())
}

/// Returns a days/hours/minutes/seconds string from given `t` seconds.
pub fn pretty_time(t: u64) -> String {
    let seconds = t % 60;
    let minutes = (t / 60) % 60;
    let hours = (t / (60 * 60)) % 24;
    let days = t / (60 * 60 * 24);

    format!("{} days, {} hours, {} minutes, {} seconds", days, hours, minutes, seconds)
}

pub fn calculate_available(
    now: u64,
    end: u64,
    ix: CreateParams,
    total: u64,
    withdrawn: u64,
    fee_percentage: f32,
) -> Result<u64, ProgramError> {
    if ix.start_time > now || ix.cliff > now || total == 0 || total == withdrawn {
        return Ok(0)
    }

    if now > end {
        return Ok(total.try_sub(withdrawn)?)
    }

    let stream_available = calculate_fee_from_amount(ix.stream_available(now)?, fee_percentage);
    let cliff_available = calculate_fee_from_amount(ix.cliff_amount, fee_percentage);
    Ok(stream_available.try_add(cliff_available)?.try_sub(withdrawn)?)
}

// TODO: impl calculations from ix
pub fn calculate_external_deposit(
    balance: u64,
    deposited: u64,
    withdrawn: u64,
) -> Result<u64, ProgramError> {
    if deposited.try_sub(withdrawn)? >= balance {
        return Ok(0)
    }

    Ok(balance.try_sub(deposited.try_sub(withdrawn)?)?)
}

/// Given amount and percentage, return the u64 of that percentage.
pub fn calculate_fee_from_amount(amount: u64, percentage: f32) -> u64 {
    if percentage <= 0.0 {
        return 0
    }
    let precision_factor: f32 = 1000000.0;
    let factor = (percentage / 100.0 * precision_factor) as u128; //largest it can get is 10^4
    (amount as u128 * factor / precision_factor as u128) as u64 // this does not fit if amount
                                                                // itself cannot fit into u64
}

pub enum Invoker {
    SenderRecipient,
    Sender,
    Recipient,
    StreamflowTreasury,
    Partner,
    None,
}

impl Invoker {
    pub fn new(
        authority: &Pubkey,
        sender: &Pubkey,
        recipient: &Pubkey,
        streamflow_treasury: &Pubkey,
        partner: &Pubkey,
    ) -> Self {
        if authority == sender && authority == recipient {
            Self::SenderRecipient
        } else if authority == recipient {
            Self::Recipient
        } else if authority == sender {
            Self::Sender
        } else if authority == streamflow_treasury {
            Self::StreamflowTreasury
        } else if authority == partner {
            Self::Partner
        } else {
            Self::None
        }
    }

    pub fn can_cancel(&self, ix: &CreateParams) -> bool {
        match self {
            Self::SenderRecipient => ix.cancelable_by_recipient || ix.cancelable_by_sender,
            Self::Sender => ix.cancelable_by_sender,
            Self::Recipient => ix.cancelable_by_recipient,
            Self::StreamflowTreasury => false,
            Self::Partner => false,
            Self::None => false,
        }
    }

    pub fn can_transfer(&self, ix: &CreateParams) -> bool {
        match self {
            Self::SenderRecipient => ix.transferable_by_sender || ix.transferable_by_recipient,
            Self::Sender => ix.transferable_by_sender,
            Self::Recipient => ix.transferable_by_recipient,
            Self::StreamflowTreasury => false,
            Self::Partner => false,
            Self::None => false,
        }
    }

    pub fn can_withdraw(&self, automatic_withdrawal: bool, requested_amount: u64) -> bool {
        if automatic_withdrawal {
            return true
        }

        match self {
            Self::SenderRecipient => true,
            Self::Sender => false,
            Self::Recipient => true,
            Self::StreamflowTreasury => requested_amount == 0,
            Self::Partner => requested_amount == 0,
            Self::None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_sanity() {
        // now, start, end, cliff
        assert!(duration_sanity(100, 110, 120).is_ok());
        assert!(duration_sanity(100, 110, 0).is_ok());
        assert!(duration_sanity(100, 140, 130).is_err());
        assert!(duration_sanity(100, 130, 130).is_ok());
        assert!(duration_sanity(130, 130, 130).is_err());
        assert!(duration_sanity(100, 110, 140).is_ok());
    }

    #[test]
    fn test_external_deposit() {
        assert_eq!(calculate_external_deposit(9, 10, 4).unwrap(), 3);
        assert_eq!(calculate_external_deposit(100, 100, 0).unwrap(), 0);
        assert_eq!(calculate_external_deposit(100, 100, 100).unwrap(), 100);
    }

    #[test]
    fn test_calculate_available() -> Result<(), ProgramError> {
        let start = 1643273913;
        let mut ix = CreateParams {
            start_time: start,
            net_amount_deposited: 100000,
            period: 1,
            amount_per_period: 1000,
            cliff: 1643273913,
            cliff_amount: 0,
            ..Default::default()
        };
        let mut end = ix.calculate_end_time()?;
        assert_eq!(calculate_available(start - 10, end, ix.clone(), 100000, 0, 100.0)?, 0);
        assert_eq!(calculate_available(start + 10, end, ix.clone(), 100000, 0, 100.0)?, 10000);
        assert_eq!(calculate_available(start + 10, end, ix.clone(), 100000, 0, 10.0)?, 1000);
        assert_eq!(calculate_available(start + 10, end, ix.clone(), 100000, 500, 10.0)?, 500);
        assert_eq!(calculate_available(end, end, ix.clone(), 100000, 0, 100.0)?, 100000);

        ix = CreateParams {
            start_time: start,
            net_amount_deposited: 100000,
            period: 1,
            amount_per_period: 1000,
            cliff: start + 20,
            cliff_amount: 10000,
            ..Default::default()
        };
        end = ix.calculate_end_time()?;
        assert_eq!(calculate_available(start + 20, end, ix.clone(), 100000, 0, 100.0)?, 10000);
        assert_eq!(calculate_available(start, end, ix.clone(), 100000, 0, 100.0)?, 0);
        assert_eq!(calculate_available(start + 10, end, ix.clone(), 100000, 0, 100.0)?, 0);
        assert_eq!(calculate_available(start + 30, end, ix.clone(), 100000, 5000, 100.0)?, 15000);
        assert_eq!(calculate_available(start + 30, end, ix.clone(), 100000, 50, 1.0)?, 150);
        Ok(())
    }
}

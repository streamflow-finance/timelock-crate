// Copyright (c) 2021 Ivan Jelincic <parazyd@dyne.org>
//
// This file is part of streamflow-finance/timelock-crate
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License version 3
// as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.
use std::iter::FromIterator;

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    program_pack::Pack,
};

use crate::{error::SfError, state::StreamInstruction};

/// Do a sanity check with given Unix timestamps.
pub fn duration_sanity(now: u64, start: u64, end: u64, cliff: u64) -> ProgramResult {
    let cliff_cond = if cliff == 0 { true } else { start <= cliff && cliff <= end };

    if now < start && start < end && cliff_cond {
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

/// Encode given amount to a string with given decimal places.
pub fn encode_base10(amount: u64, decimal_places: usize) -> String {
    let mut s: Vec<char> =
        format!("{:0width$}", amount, width = 1 + decimal_places).chars().collect();
    s.insert(s.len() - decimal_places, '.');

    String::from_iter(&s).trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn calculate_available(now: u64, ix: StreamInstruction, total: u64, withdrawn: u64) -> u64 {
    if ix.start_time > now || ix.cliff > now {
        return 0
    }

    // Ignore end date when recurring
    if now > ix.end_time && ix.release_rate == 0 {
        return total - withdrawn
    }

    let cliff = if ix.cliff > 0 { ix.cliff } else { ix.start_time };
    let cliff_amount = if ix.cliff_amount > 0 { ix.cliff_amount } else { 0 };

    // TODO: Use uint arithmetics
    let num_periods = (ix.end_time - cliff) as f64 / ix.period as f64;
    let period_amount = if ix.release_rate > 0 {
        ix.release_rate as f64
    } else {
        (ix.total_amount - cliff_amount) as f64 / num_periods
    };

    let periods_passed = (now - cliff) / ix.period;
    (periods_passed as f64 * period_amount) as u64 + cliff_amount - withdrawn
}

#[allow(unused_imports)]
mod tests {
    use crate::utils::duration_sanity;

    #[test]
    fn test_duration_sanity() {
        // now, start, end, cliff
        assert!(duration_sanity(100, 110, 130, 120).is_ok());
        assert!(duration_sanity(100, 110, 130, 0).is_ok());
        assert!(duration_sanity(100, 140, 130, 130).is_err());
        assert!(duration_sanity(100, 130, 130, 130).is_err());
        assert!(duration_sanity(130, 130, 130, 130).is_err());
        assert!(duration_sanity(100, 110, 130, 140).is_err());
    }
}

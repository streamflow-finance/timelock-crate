// Copyright (c) 2021 Ivan Jelincic <parazyd@dyne.org>
//
// This file is part of streamflow-timelock
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

use solana_program::{account_info::AccountInfo, program_error::ProgramError, program_pack::Pack};

/// Do a sanity check with given Unix timestamps.
pub fn duration_sanity(now: u64, start: u64, end: u64, cliff: u64) -> bool {
    let cliff_cond = if cliff == 0 {
        true
    } else {
        start <= cliff && cliff <= end
    };

    now < start && start < end && cliff_cond
}

/// Unpack token account from `account_info`
pub fn unpack_token_account(
    account_info: &AccountInfo,
) -> Result<spl_token::state::Account, ProgramError> {
    if account_info.owner != &spl_token::id() {
        return Err(ProgramError::InvalidAccountData);
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

    format!(
        "{} days, {} hours, {} minutes, {} seconds",
        days, hours, minutes, seconds
    )
}

pub fn encode_base10(amount: u64, decimal_places: usize) -> String {
    let mut s: Vec<char> = format!("{:0width$}", amount, width = 1 + decimal_places)
        .chars()
        .collect();
    s.insert(s.len() - decimal_places, '.');

    String::from_iter(&s)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

#[allow(unused_imports)]
mod tests {
    use crate::utils::duration_sanity;

    #[test]
    fn test_duration_sanity() {
        // now, start, end, cliff
        assert_eq!(true, duration_sanity(100, 110, 130, 120));
        assert_eq!(true, duration_sanity(100, 110, 130, 0));
        assert_eq!(false, duration_sanity(100, 140, 130, 130));
        assert_eq!(false, duration_sanity(100, 130, 130, 130));
        assert_eq!(false, duration_sanity(130, 130, 130, 130));
        assert_eq!(false, duration_sanity(100, 110, 130, 140));
    }
}

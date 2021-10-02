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
use solana_program::{account_info::AccountInfo, program_error::ProgramError, program_pack::Pack};

/// Do a sanity check with given Unix timestamps.
pub fn duration_sanity(now: u64, start: u64, end: u64) -> bool {
    !(start < now || start >= end)
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

/// Unpack mint account from `accunt_info`
pub fn unpack_mint_account(
    account_info: &AccountInfo,
) -> Result<spl_token::state::Mint, ProgramError> {
    spl_token::state::Mint::unpack(&account_info.data.borrow())
}

/// Returns a hours/minutes/seconds string from given `t` seconds.
pub fn pretty_time(t: u64) -> String {
    let seconds = t % 60;
    let minutes = (t / 60) % 60;
    let hours = (t / 60) / 60;

    format!("{} hours, {} minutes, {} seconds", hours, minutes, seconds)
}

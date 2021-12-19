// Copyright (c) 2021 Streamflow Labs Limited <legal@streamflowlabs.com>
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

//! The code providing timelock primitives
//! used by [streamflow.finance](https://streamflow.finance). This is a free and open-source community version of [Streamflow Timelock](https://github.com/streamflow-finance/timelock-crate) protocol, that comes with certain limitations compared to the commercial version.
//!
//! This Rust crate provides SPL timelock functionalities that can be used "out of the box" and integrated in other Solana programs.
//!
//! Functionalities are:
//! - `create` a vesting contract.
//! - `withdraw` from a vesting contract. _Invoked by recipient (beneficiary)_
//! - `cancel` a vesting contract. _Invoked by sender (creator)_
//! - `transfer_recipient` of a vesting contract. _Invoked by recipient (beneficiary)_
//!
//! UI is available at https://app.streamflow.finance/vesting

/// Structs and data
pub mod state;
/// Functions related to SPL tokens
pub mod token;
/// Utility functions
pub mod utils;

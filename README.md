Streamflow
---
_Disclaimer: This is a Community (free and open-source) version of a [Streamflow protocol](https://github.com/streamflow-finance/js-sdk). It has limited set of features and is provided as is, without support._

_Reference implementation (also free and open-source, implemented in Anchor) that uses this protocol (as crate) is available [here](https://github.com/streamflow-finance/js-sdk/tree/community). That program is deployed on Solana mainnet with the program ID: `8e72pYCDaxu3GqMfeQ5r8wFgoZSYk6oua1Qo9XpsZjX`_

**To interact with Streamflow protocol (commercial version with full feature set), you can use [the application](https://app.streamflow.finance?utm_medium=github.com&utm_source=referral&utm_campaign=timelock-crate-repo), [JS SDK](https://github.com/streamflow-finance/js-sdk) or [Rust SDK](https://github.com/streamflow-finance/rust-sdk).**

---
**Security audit passed. [Report here.](https://github.com/StreamFlow-Finance/timelock-crate/blob/master/TIMELOCK_COMMUNITY_REPORT_FINAL.pdf) ✅**

This Rust crate provides SPL timelock functionalities that can be used "out of the box" and integrated in other Solana programs.

Functionalities are:
- `create` a vesting contract.
- `withdraw` from a vesting contract.
- `cancel` a vesting contract.
- `transfer_recipient` of a vesting contract.

High level overview
--
![Overview](/misc/overview.jpeg)

Check the [docs](https://docs.rs/streamflow-timelock/) to get familiar with the crate.
Or check the [reference program implementation](https://github.com/streamflow-finance/js-sdk/tree/community) where this crate is used.

Run `python3 misc/make_idl.py > OUTPUT_FILE.js` to generate JS IDL to be used for easy (de)serialization of the program account data structs.

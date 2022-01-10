streamflow-timelock
===================
**Security audit passed. [Report here.](https://github.com/StreamFlow-Finance/timelock-crate/blob/community/TIMELOCK_COMMUNITY_REPORT_FINAL.pdf) ✅**

This is a free and open-source community version of [Streamflow Timelock](../../tree/master) protocol, that comes with certain limitations compared to the commercial version.

This Rust crate provides SPL timelock functionalities that can be used "out of the box" and integrated in other Solana programs.

Functionalities are:
- `create` a vesting contract.
- `withdraw` from a vesting contract. _Invoked by recipient (beneficiary)_
- `cancel` a vesting contract. _Invoked by sender (creator)_
- `transfer_recipient` of a vesting contract. _Invoked by recipient (beneficiary)_

UI is available at https://app.streamflow.finance/vesting

High level overview
--
![Overview](/misc/overview.jpeg)

Check the [docs](https://docs.rs/streamflow-timelock/) to get familiar with the crate.
Or check the [Timelock program implementation](https://github.com/streamflow-finance/timelock) where this crate is used.

Run `python3 misc/gen_js_api.py > OUTPUT_FILE.js` to generate JS layout to be used for easy (de)serialization of the program account data structs.

License
-------

`timelock-crate` is licensed [AGPL-3](LICENSE).

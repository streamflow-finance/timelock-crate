streamflow-timelock
===================
**Disclaimer: The software isn't audited. No warranties provided.**

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
Or check the [Timelock program implementation](https://github.com/streamflow-finance/timelock) where this crate is used.

Run `python3 misc/gen_js_api.py > OUTPUT_FILE.js` to generate JS IDL to be used for easy (de)serialization of the program account data structs.

License
-------

`timelock-crate` is licensed [AGPL-3](LICENSE).
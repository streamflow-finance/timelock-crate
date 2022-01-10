streamflow-timelock
===================
**Security audit passed. [Report here.](https://github.com/StreamFlow-Finance/timelock-crate/blob/community/TIMELOCK_IMPLEMENTATION_COMMUNITY_REPORT_FINAL.pdf) ✅**

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

Run `python3 misc/make_idl.py > OUTPUT_FILE.js` to generate JS IDL to be used for easy (de)serialization of the program account data structs.

License
-------
`timelock-crate` is licensed under [Business Source License](LICENSE).
The [Business Source License](LICENSE) is not a Free and Open-source license. However, the Licensed Work will eventually be made available
under an Open Source License, as stated in this License.

For the community (free and open-source) version, please see [this release](https://github.com/StreamFlow-Finance/timelock-crate/releases/tag/v0.3.0).

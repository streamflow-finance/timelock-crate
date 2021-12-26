use borsh::BorshSerialize;
use solana_program::{
    account_info::AccountInfo,
    borsh as solana_borsh,
    entrypoint::ProgramResult,
    msg,
    program::invoke,
    pubkey::Pubkey,
    sysvar::{clock::Clock, Sysvar},
};
use spl_token::amount_to_ui_amount;
use spl_token::solana_program::program_pack::Pack;

use crate::{
    error::SfError,
    state::Contract,
    utils::{calculate_fee_from_amount, unpack_mint_account},
};

#[derive(Clone, Debug)]
pub struct TopupAccounts<'a> {
    pub sender: AccountInfo<'a>,
    pub sender_tokens: AccountInfo<'a>,
    pub metadata: AccountInfo<'a>,
    pub escrow_tokens: AccountInfo<'a>,
    pub streamflow_treasury: AccountInfo<'a>,
    pub streamflow_treasury_tokens: AccountInfo<'a>,
    pub partner: AccountInfo<'a>,
    pub partner_tokens: AccountInfo<'a>,
    pub mint: AccountInfo<'a>,
    pub token_program: AccountInfo<'a>,
}

pub fn topup(_program_id: &Pubkey, acc: TopupAccounts, amount: u64) -> ProgramResult {
    msg!("Topping up escrow account");

    // Sanity checks
    //account_sanity_check

    if amount == 0 {
        return Err(SfError::AmountIsZero.into())
    }

    let mut data = acc.metadata.try_borrow_mut_data()?;
    let mut metadata: Contract = match solana_borsh::try_from_slice_unchecked(&data) {
        Ok(v) => v,
        Err(_) => return Err(SfError::InvalidMetadata.into()),
    };

    let escrow_tokens = spl_token::state::Account::unpack_from_slice(&acc.escrow_tokens.data.borrow())?;

    metadata.sync_balance(escrow_tokens.amount);

    //metadata_sanity_check(acc.clone())?;

    let now = Clock::get()?.unix_timestamp as u64;
    if metadata.closable() < now {
        return Err(SfError::StreamClosed.into())
    }

    msg!("Transferring funds into escrow account");
    invoke(
        &spl_token::instruction::transfer(
            acc.token_program.key,
            acc.sender_tokens.key,
            acc.escrow_tokens.key,
            acc.sender.key,
            &[],
            amount,
        )?,
        &[
            acc.sender_tokens.clone(),
            acc.escrow_tokens.clone(),
            acc.sender.clone(),
            acc.token_program.clone(),
        ],
    )?;

    metadata.deposit(amount);
    metadata.closable_at = metadata.closable();

    let bytes = metadata.try_to_vec()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    let mint_info = unpack_mint_account(&acc.mint)?;

    msg!(
        "Successfully topped up {} to token stream {} on behalf of {}",
        amount_to_ui_amount(amount, mint_info.decimals),
        acc.escrow_tokens.key,
        acc.sender.key,
    );

    Ok(())
}

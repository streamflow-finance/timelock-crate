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

use crate::{
    error::SfError,
    state::TokenStreamData,
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
    let mut metadata: TokenStreamData = match solana_borsh::try_from_slice_unchecked(&data) {
        Ok(v) => v,
        Err(_) => return Err(SfError::InvalidMetadata.into()),
    };

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

    let mint_info = unpack_mint_account(&acc.mint)?;
    let uint_fee_for_partner = calculate_fee_from_amount(amount, metadata.partner_fee_percent);
    let uint_fee_for_strm = calculate_fee_from_amount(amount, metadata.streamflow_fee_percent);
    msg!("Fee for partner: {}", uint_fee_for_partner / mint_info.decimals as u64);
    msg!("Fee for Streamflow: {}", uint_fee_for_strm / mint_info.decimals as u64);

    // TODO: Do we request topup + fees, or take fees from the topup?
    metadata.streamflow_fee_total += uint_fee_for_strm;
    metadata.partner_fee_total += uint_fee_for_partner;
    metadata.ix.deposited_amount += amount - uint_fee_for_strm - uint_fee_for_partner;
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

use borsh::BorshSerialize;
use num_traits::FromPrimitive;
use solana_program::{
    borsh as solana_borsh,
    entrypoint::ProgramResult,
    msg,
    program::invoke,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar::{clock::Clock, Sysvar},
};

use crate::{
    error::SfError,
    state::{InstructionAccounts, TokenStreamData},
    stream_safety::{initialized_account_sanity_check, metadata_sanity_check},
    utils::{encode_base10, unpack_mint_account},
};

pub fn topup(program_id: &Pubkey, acc: InstructionAccounts, amount: u64) -> ProgramResult {
    msg!("Topping up escrow account");

    if !acc.sender.is_signer {
        return Err(ProgramError::MissingRequiredSignature)
    }

    // Sanity checks
    initialized_account_sanity_check(program_id, acc.clone())?;
    metadata_sanity_check(acc.clone())?;

    if amount == 0 {
        return Err(SfError::AmountIsZero.into())
    }

    let mut data = acc.metadata.try_borrow_mut_data()?;
    let mut metadata: TokenStreamData = match solana_borsh::try_from_slice_unchecked(&data) {
        Ok(v) => v,
        Err(_) => return Err(SfError::InvalidMetadata.into()),
    };

    let now = Clock::get()?.unix_timestamp as u64;
    if metadata.closable() < now {
        return Err(SfError::StreamClosed.into())
    }

    // TODO: Do we request topup + fees, or take fees from the topup?

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

    let mut uint_fee_for_partner: u64 = 0;
    if metadata.partner_fee_percent > 0.0 {
        // TODO: Test units, and generic function
        let fee_for_partner = amount as f64 * (metadata.partner_fee_percent / 100.0) as f64;
        msg!("Fee for partner: {}", fee_for_partner);
        let r = fee_for_partner * f64::from_u8(mint_info.decimals).unwrap().floor();
        uint_fee_for_partner = r as u64;
    }

    let mut uint_fee_for_strm: u64 = 0;
    if metadata.streamflow_fee_percent > 0.0 {
        // TODO: Test units, and generic function
        let fee_for_strm = amount as f64 * (metadata.streamflow_fee_percent / 100.0) as f64;
        msg!("Fee for Streamflow: {}", fee_for_strm);
        let r = fee_for_strm * f64::from_u8(mint_info.decimals).unwrap().floor();
        uint_fee_for_strm = r as u64;
    }

    metadata.streamflow_fee_total += uint_fee_for_strm;
    metadata.partner_fee_total += uint_fee_for_partner;
    metadata.ix.deposited_amount += amount - uint_fee_for_strm - uint_fee_for_partner;
    metadata.closable_at = metadata.closable();

    let bytes = metadata.try_to_vec()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    let mint_info = unpack_mint_account(&acc.mint)?;

    msg!(
        "Successfully topped up {} to token stream {} on behalf of {}",
        encode_base10(amount, mint_info.decimals.into()),
        acc.escrow_tokens.key,
        acc.sender.key,
    );

    Ok(())
}

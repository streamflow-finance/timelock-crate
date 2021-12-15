use borsh::BorshSerialize;
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

pub(crate) fn topup(program_id: &Pubkey, acc: InstructionAccounts, amount: u64) -> ProgramResult {
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

    msg!("Transferring funds into escrow account");
    // TODO: Fees
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

    metadata.ix.deposited_amount += amount;
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

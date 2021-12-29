use crate::{
    state::Contract,
    utils::{calculate_external_deposit, unpack_token_account},
};
use solana_program::{
    account_info::AccountInfo, msg, program::invoke_signed, program_error::ProgramError,
};

pub fn close_escrow<'a>(
    metadata: &Contract,
    seeds: &[&[u8]],
    token_program: &AccountInfo<'a>,
    escrow_tokens: &AccountInfo<'a>,
    streamflow_treasury: &AccountInfo<'a>,
    streamflow_treasury_tokens: &AccountInfo<'a>,
) -> Result<(), ProgramError> {
    let escrow_tokens_spl_account = unpack_token_account(escrow_tokens)?;
    let external_deposit = calculate_external_deposit(
        escrow_tokens_spl_account.amount,
        metadata.gross_amount(),
        metadata.amount_withdrawn,
    );

    msg!(
        "Transferring leftover external deposit: {} tokens to Streamflow treasury",
        metadata.gross_amount()
    );
    invoke_signed(
        &spl_token::instruction::transfer(
            token_program.key,
            escrow_tokens.key,
            streamflow_treasury_tokens.key,
            escrow_tokens.key,
            &[],
            external_deposit,
        )?,
        &[
            escrow_tokens.clone(),              // src
            streamflow_treasury_tokens.clone(), // dest
            escrow_tokens.clone(),              // auth
            token_program.clone(),              // program
        ],
        &[seeds],
    )?;

    msg!("Closing escrow SPL token account");
    invoke_signed(
        &spl_token::instruction::close_account(
            token_program.key,
            escrow_tokens.key,
            streamflow_treasury.key,
            escrow_tokens.key,
            &[],
        )?,
        &[escrow_tokens.clone(), streamflow_treasury.clone(), escrow_tokens.clone()],
        &[seeds],
    )?;
    Ok(())
}

use solana_program::{
    account_info::AccountInfo,
    borsh as solana_borsh,
    entrypoint::ProgramResult,
    msg,
    program::invoke,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar::{clock::Clock, Sysvar},
};

use spl_token::amount_to_ui_amount;

use crate::{
    error::SfError,
    state::{find_escrow_account, save_account_info, Contract},
    utils::{calculate_fee_from_amount, unpack_mint_account, unpack_token_account},
};

#[derive(Clone, Debug)]
pub struct TopupAccounts<'a> {
    pub sender: AccountInfo<'a>,                     // [writable, signer]
    pub sender_tokens: AccountInfo<'a>,              // [writable]
    pub metadata: AccountInfo<'a>,                   // [writable]
    pub escrow_tokens: AccountInfo<'a>,              // [writable]
    pub streamflow_treasury: AccountInfo<'a>,        // []
    pub streamflow_treasury_tokens: AccountInfo<'a>, // [writable]
    pub partner: AccountInfo<'a>,                    // []
    pub partner_tokens: AccountInfo<'a>,             // [writable]
    pub mint: AccountInfo<'a>,                       // []
    pub token_program: AccountInfo<'a>,              // []
}

pub fn topup(pid: &Pubkey, acc: TopupAccounts, amount: u64) -> ProgramResult {
    msg!("Topping up escrow account");

    // Sanity checks
    //account_sanity_check
    if !acc.sender.is_signer {
        return Err(ProgramError::MissingRequiredSignature)
    }

    if acc.token_program.key != &spl_token::id() {
        return Err(ProgramError::InvalidAccountData)
    }

    if amount == 0 {
        return Err(SfError::AmountIsZero.into())
    }

    let data = acc.metadata.try_borrow_mut_data()?;
    let mut metadata: Contract = match solana_borsh::try_from_slice_unchecked(&data) {
        Ok(v) => v,
        Err(_) => return Err(SfError::InvalidMetadata.into()),
    };

    if !metadata.ix.can_topup {
        return Err(SfError::InvalidMetadata.into())
    }

    // Taking the protocol version from the metadata, we check that the token
    // escrow account is correct:
    if &find_escrow_account(metadata.version, acc.metadata.key.as_ref(), pid).0 !=
        acc.escrow_tokens.key
    {
        return Err(ProgramError::InvalidAccountData)
    }

    let escrow_tokens = unpack_token_account(&acc.escrow_tokens)?;
    metadata.try_sync_balance(escrow_tokens.amount)?;

    let now = Clock::get()?.unix_timestamp as u64;
    if metadata.end_time < now {
        return Err(SfError::StreamClosed.into())
    }

    let strm_fee = calculate_fee_from_amount(amount, metadata.streamflow_fee_percent);
    let partner_fee = calculate_fee_from_amount(amount, metadata.partner_fee_percent);

    msg!("Transferring funds into escrow account");
    invoke(
        &spl_token::instruction::transfer(
            acc.token_program.key,
            acc.sender_tokens.key,
            acc.escrow_tokens.key,
            acc.sender.key,
            &[],
            amount + partner_fee + strm_fee,
        )?,
        &[
            acc.sender_tokens.clone(),
            acc.escrow_tokens.clone(),
            acc.sender.clone(),
            acc.token_program.clone(),
        ],
    )?;

    metadata.deposit_net(amount)?;
    save_account_info(&metadata, data)?;

    let mint_info = unpack_mint_account(&acc.mint)?;

    msg!(
        "Successfully topped up {} to token stream {} on behalf of {}",
        amount_to_ui_amount(metadata.gross_amount()?, mint_info.decimals),
        acc.escrow_tokens.key,
        acc.sender.key,
    );

    Ok(())
}

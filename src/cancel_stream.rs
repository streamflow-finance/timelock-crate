use borsh::BorshSerialize;
use solana_program::{
    borsh as solana_borsh,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar::{clock::Clock, Sysvar},
};
use spl_associated_token_account::create_associated_token_account;

use crate::{
    error::SfError,
    state::{InstructionAccounts, TokenStreamData},
    stream_safety::{initialized_account_sanity_check, metadata_sanity_check},
    utils::{calculate_available, encode_base10, unpack_mint_account},
};

/// Cancel an SPL Token stream
///
/// The function will read the instructions from the metadata account and see
/// if there are any unlocked funds. If so, they will be transferred to the
/// stream recipient.
pub fn cancel(program_id: &Pubkey, acc: InstructionAccounts) -> ProgramResult {
    msg!("Cancelling SPL token stream");

    let now = Clock::get()?.unix_timestamp as u64;
    let mint_info = unpack_mint_account(&acc.mint)?;

    // Sanity checks
    initialized_account_sanity_check(program_id, acc.clone())?;
    metadata_sanity_check(acc.clone())?;

    let mut data = acc.metadata.try_borrow_mut_data()?;
    let mut metadata: TokenStreamData = match solana_borsh::try_from_slice_unchecked(&data) {
        Ok(v) => v,
        Err(_) => return Err(SfError::InvalidMetadata.into()),
    };

    // If stream is expired, anyone can close it
    if now < metadata.closable_at {
        msg!("Stream not yet expired, checking authorization");
        if acc.authority.key != acc.sender.key {
            return Err(ProgramError::InvalidAccountData)
        }

        if !acc.authority.is_signer {
            return Err(ProgramError::MissingRequiredSignature)
        }
    }

    let recipient_available = calculate_available(
        now,
        metadata.ix.clone(),
        metadata.ix.deposited_amount,
        metadata.withdrawn_amount,
    );

    let streamflow_available = calculate_available(
        now,
        metadata.ix.clone(),
        metadata.streamflow_fee_total,
        metadata.streamflow_fee_withdrawn,
    );

    let partner_available = calculate_available(
        now,
        metadata.ix.clone(),
        metadata.partner_fee_total,
        metadata.partner_fee_withdrawn,
    );

    // TODO: Handle requested amounts.

    let recipient_remains = metadata.ix.deposited_amount - recipient_available;
    let streamflow_remains = metadata.streamflow_fee_total - streamflow_available;
    let partner_remains = metadata.partner_fee_total - partner_available;

    let escrow_tokens_bump =
        Pubkey::find_program_address(&[acc.metadata.key.as_ref()], program_id).1;
    let seeds = [acc.metadata.key.as_ref(), &[escrow_tokens_bump]];

    if recipient_available > 0 {
        msg!("Transferring unlocked tokens to recipient");
        if acc.recipient_tokens.data_is_empty() {
            msg!("Initializing recipient's associated token account");
            invoke(
                &create_associated_token_account(acc.sender.key, acc.recipient.key, acc.mint.key),
                &[
                    acc.sender.clone(),
                    acc.recipient_tokens.clone(),
                    acc.recipient.clone(),
                    acc.mint.clone(),
                    acc.system_program.clone(),
                    acc.token_program.clone(),
                    acc.rent.clone(),
                ],
            )?;
        }

        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.recipient_tokens.key,
                acc.escrow_tokens.key,
                &[],
                recipient_available, // TODO: FIXME
            )?,
            &[
                acc.escrow_tokens.clone(),    // src
                acc.recipient_tokens.clone(), // dest
                acc.escrow_tokens.clone(),    // auth
                acc.token_program.clone(),    // program
            ],
            &[&seeds],
        )?;

        metadata.withdrawn_amount += recipient_available; // TODO: FIXME
        metadata.last_withdrawn_at = now;
        msg!(
            "Withdrawn: {} {} tokens",
            encode_base10(recipient_available, mint_info.decimals.into()),
            metadata.mint
        );
        msg!(
            "Remaining: {} {} tokens",
            encode_base10(
                metadata.ix.deposited_amount - metadata.withdrawn_amount,
                mint_info.decimals.into()
            ),
            metadata.mint
        );
    }

    if streamflow_available > 0 {
        msg!("Transferring unlocked tokens to Streamflow treasury");
        if acc.streamflow_treasury_tokens.data_is_empty() {
            msg!("Initializing Streamflow treasury associated token account");
            invoke(
                &create_associated_token_account(
                    acc.sender.key,
                    acc.streamflow_treasury.key,
                    acc.mint.key,
                ),
                &[
                    acc.sender.clone(),
                    acc.streamflow_treasury_tokens.clone(),
                    acc.streamflow_treasury.clone(),
                    acc.mint.clone(),
                    acc.system_program.clone(),
                    acc.token_program.clone(),
                    acc.rent.clone(),
                ],
            )?;
        }

        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.streamflow_treasury_tokens.key,
                acc.escrow_tokens.key,
                &[],
                streamflow_available, // TODO: FIXME
            )?,
            &[
                acc.escrow_tokens.clone(),              // src
                acc.streamflow_treasury_tokens.clone(), // dest
                acc.escrow_tokens.clone(),              // auth
                acc.token_program.clone(),              // program
            ],
            &[&seeds],
        )?;

        metadata.streamflow_fee_withdrawn += streamflow_available; // TODO: FIXME
        metadata.last_withdrawn_at = now;
        msg!(
            "Withdrawn: {} {} tokens",
            encode_base10(streamflow_available, mint_info.decimals.into()),
            metadata.mint
        );
        msg!(
            "Remaining: {} {} tokens",
            encode_base10(
                metadata.streamflow_fee_total - metadata.streamflow_fee_withdrawn,
                mint_info.decimals.into()
            ),
            metadata.mint
        );
    }

    if partner_available > 0 {
        msg!("Transferring unlocked tokens to partner");
        if acc.partner_tokens.data_is_empty() {
            msg!("Initializing parther's associated token account");
            invoke(
                &create_associated_token_account(acc.sender.key, acc.partner.key, acc.mint.key),
                &[
                    acc.sender.clone(),
                    acc.partner_tokens.clone(),
                    acc.partner.clone(),
                    acc.mint.clone(),
                    acc.system_program.clone(),
                    acc.token_program.clone(),
                    acc.rent.clone(),
                ],
            )?;
        }

        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.partner_tokens.key,
                acc.escrow_tokens.key,
                &[],
                partner_available, // TODO: FIXME
            )?,
            &[
                acc.escrow_tokens.clone(),  // src
                acc.partner_tokens.clone(), // dest
                acc.escrow_tokens.clone(),  // auth
                acc.token_program.clone(),  // program
            ],
            &[&seeds],
        )?;

        metadata.partner_fee_withdrawn += partner_available; // TODO: FIXME
        metadata.last_withdrawn_at = now;
        msg!(
            "Withdrawn: {} {} tokens",
            encode_base10(partner_available, mint_info.decimals.into()),
            metadata.mint
        );
        msg!(
            "Remaining: {} {} tokens",
            encode_base10(
                metadata.partner_fee_total - metadata.partner_fee_withdrawn,
                mint_info.decimals.into()
            ),
            metadata.mint
        );
    }

    if recipient_remains > 0 || streamflow_remains > 0 || partner_remains > 0 {
        msg!("Transferring remains back to sender");
        invoke_signed(
            &spl_token::instruction::transfer(
                acc.token_program.key,
                acc.escrow_tokens.key,
                acc.sender_tokens.key,
                acc.escrow_tokens.key,
                &[],
                recipient_remains + streamflow_remains + partner_remains,
            )?,
            &[
                acc.escrow_tokens.clone(), // src
                acc.sender_tokens.clone(), // dest
                acc.escrow_tokens.clone(), // auth
                acc.token_program.clone(), // program
            ],
            &[&seeds],
        )?;
    }

    // TODO: Close metadata account once there is an alternative storage
    // solution for historical data.
    msg!("Closing escrow account");
    invoke_signed(
        &spl_token::instruction::close_account(
            acc.token_program.key,
            acc.escrow_tokens.key,
            acc.streamflow_treasury.key,
            acc.escrow_tokens.key,
            &[],
        )?,
        &[acc.escrow_tokens.clone(), acc.streamflow_treasury.clone(), acc.escrow_tokens.clone()],
        &[&seeds],
    )?;

    if now < metadata.closable_at {
        metadata.last_withdrawn_at = now;
        metadata.canceled_at = now;
    }

    // Write the metadata to the account
    let bytes = metadata.try_to_vec()?;
    data[0..bytes.len()].clone_from_slice(&bytes);

    Ok(())
}

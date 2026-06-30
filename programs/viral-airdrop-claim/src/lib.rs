//! viral-airdrop-claim — Merkle-drop distribution for the verifiable viral airdrop.
//!
//! Pairs with `viral-airdrop/` (TS). The off-chain tool produces a claim Merkle
//! tree from the PRUV allocation; this program lets each recipient claim exactly
//! once. Leaf/node hashing is byte-for-byte identical to `claim-tree.ts`:
//!
//!   leaf = sha256( 0x00 || u64LE(index) || claimant_pubkey[32] || u64LE(amount) )
//!   node = sha256( 0x01 || min(a,b) || max(a,b) )         // sorted pair
//!
//! Flow:
//!   1. `initialize(merkle_root)` — authority creates the Distributor PDA + a
//!      vault token account it controls, then funds the vault (plain SPL transfer).
//!   2. `claim(index, amount, proof)` — caller proves their leaf against the root;
//!      a per-index ClaimStatus PDA (created with `init`) is the nullifier, so a
//!      second claim for the same index fails. Tokens move vault -> claimant.

use anchor_lang::prelude::*;
use solana_program::hash::hashv;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

// Placeholder id — run `anchor keys sync` before deploying.
declare_id!("3oCMjxiXMorGrFUrFqYUmpfwG1FMLaLBWJBh6pVRcLqJ");

#[program]
pub mod viral_airdrop_claim {
    use super::*;

    /// Commit-before-seed. Records the SNAPSHOT root and a FUTURE seed slot
    /// before the allocation randomness is known. The seed = slot hash at
    /// `seed_slot` (unknowable now), so the operator cannot steer the lottery.
    /// Anyone can later read (snapshot_root, seed_slot), fetch the slot hash,
    /// and recompute the weighted-lottery allocation off-chain to verify it.
    pub fn commit_snapshot(
        ctx: Context<CommitSnapshot>,
        snapshot_root: [u8; 32],
        seed_slot: u64,
    ) -> Result<()> {
        let clock = Clock::get()?;
        require!(seed_slot > clock.slot, ClaimError::SeedSlotNotFuture);
        let c = &mut ctx.accounts.commitment;
        c.authority = ctx.accounts.authority.key();
        c.snapshot_root = snapshot_root;
        c.seed_slot = seed_slot;
        c.committed_slot = clock.slot;
        c.bump = ctx.bumps.commitment;
        emit!(SnapshotCommitted { snapshot_root, seed_slot, committed_slot: clock.slot });
        Ok(())
    }

    pub fn initialize(ctx: Context<Initialize>, merkle_root: [u8; 32]) -> Result<()> {
        let d = &mut ctx.accounts.distributor;
        d.authority = ctx.accounts.authority.key();
        d.mint = ctx.accounts.mint.key();
        d.merkle_root = merkle_root;
        d.claimed_count = 0;
        d.total_claimed = 0;
        d.bump = ctx.bumps.distributor;
        d.vault_bump = ctx.bumps.vault;
        Ok(())
    }

    pub fn claim(
        ctx: Context<Claim>,
        index: u64,
        amount: u64,
        proof: Vec<[u8; 32]>,
    ) -> Result<()> {
        // Recompute the leaf for THIS signer (binds the award to the caller).
        let leaf = hashv(&[
            &[0u8][..],
            &index.to_le_bytes()[..],
            ctx.accounts.claimant.key().as_ref(),
            &amount.to_le_bytes()[..],
        ])
        .to_bytes();

        // Fold the proof with sorted-pair hashing (direction-independent).
        let mut computed = leaf;
        for sib in proof.iter() {
            computed = if computed <= *sib {
                hashv(&[&[1u8][..], &computed[..], &sib[..]]).to_bytes()
            } else {
                hashv(&[&[1u8][..], &sib[..], &computed[..]]).to_bytes()
            };
        }
        require!(
            computed == ctx.accounts.distributor.merkle_root,
            ClaimError::InvalidProof
        );

        // Transfer vault -> claimant, signed by the Distributor PDA.
        let merkle_root = ctx.accounts.distributor.merkle_root;
        let bump = ctx.accounts.distributor.bump;
        let signer_seeds: &[&[&[u8]]] = &[&[b"distributor", merkle_root.as_ref(), &[bump]]];
        let cpi = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.vault.to_account_info(),
                to: ctx.accounts.claimant_token.to_account_info(),
                authority: ctx.accounts.distributor.to_account_info(),
            },
            signer_seeds,
        );
        token::transfer(cpi, amount)?;

        // Record + accounting. The ClaimStatus PDA was created with `init`,
        // so re-claiming the same index aborts before reaching here.
        let d = &mut ctx.accounts.distributor;
        d.claimed_count = d.claimed_count.checked_add(1).unwrap();
        d.total_claimed = d.total_claimed.checked_add(amount).unwrap();

        let cs = &mut ctx.accounts.claim_status;
        cs.claimant = ctx.accounts.claimant.key();
        cs.index = index;
        cs.amount = amount;

        emit!(Claimed { claimant: cs.claimant, index, amount });
        Ok(())
    }
}

// ─── Accounts ───────────────────────────────────────────────────────────────

#[derive(Accounts)]
#[instruction(snapshot_root: [u8; 32])]
pub struct CommitSnapshot<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init,
        payer = authority,
        space = 8 + SnapshotCommitment::LEN,
        seeds = [b"commitment", snapshot_root.as_ref()],
        bump
    )]
    pub commitment: Account<'info, SnapshotCommitment>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(merkle_root: [u8; 32])]
pub struct Initialize<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    pub mint: Account<'info, Mint>,
    #[account(
        init,
        payer = authority,
        space = 8 + Distributor::LEN,
        seeds = [b"distributor", merkle_root.as_ref()],
        bump
    )]
    pub distributor: Account<'info, Distributor>,
    #[account(
        init,
        payer = authority,
        seeds = [b"vault", distributor.key().as_ref()],
        bump,
        token::mint = mint,
        token::authority = distributor
    )]
    pub vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(index: u64, amount: u64)]
pub struct Claim<'info> {
    #[account(mut)]
    pub claimant: Signer<'info>,
    #[account(
        mut,
        seeds = [b"distributor", distributor.merkle_root.as_ref()],
        bump = distributor.bump
    )]
    pub distributor: Account<'info, Distributor>,
    #[account(
        mut,
        seeds = [b"vault", distributor.key().as_ref()],
        bump = distributor.vault_bump
    )]
    pub vault: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = claimant_token.mint == distributor.mint @ ClaimError::WrongMint,
        constraint = claimant_token.owner == claimant.key() @ ClaimError::WrongOwner
    )]
    pub claimant_token: Account<'info, TokenAccount>,
    #[account(
        init,
        payer = claimant,
        space = 8 + ClaimStatus::LEN,
        seeds = [b"claim", distributor.key().as_ref(), index.to_le_bytes().as_ref()],
        bump
    )]
    pub claim_status: Account<'info, ClaimStatus>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

// ─── State ──────────────────────────────────────────────────────────────────

#[account]
pub struct SnapshotCommitment {
    pub authority: Pubkey,
    pub snapshot_root: [u8; 32],
    pub seed_slot: u64,
    pub committed_slot: u64,
    pub bump: u8,
}
impl SnapshotCommitment {
    pub const LEN: usize = 32 + 32 + 8 + 8 + 1;
}

#[account]
pub struct Distributor {
    pub authority: Pubkey,
    pub mint: Pubkey,
    pub merkle_root: [u8; 32],
    pub claimed_count: u64,
    pub total_claimed: u64,
    pub bump: u8,
    pub vault_bump: u8,
}
impl Distributor {
    pub const LEN: usize = 32 + 32 + 32 + 8 + 8 + 1 + 1;
}

#[account]
pub struct ClaimStatus {
    pub claimant: Pubkey,
    pub index: u64,
    pub amount: u64,
}
impl ClaimStatus {
    pub const LEN: usize = 32 + 8 + 8;
}

#[event]
pub struct Claimed {
    pub claimant: Pubkey,
    pub index: u64,
    pub amount: u64,
}

#[event]
pub struct SnapshotCommitted {
    pub snapshot_root: [u8; 32],
    pub seed_slot: u64,
    pub committed_slot: u64,
}

#[error_code]
pub enum ClaimError {
    #[msg("Invalid Merkle proof")]
    InvalidProof,
    #[msg("Claimant token account mint mismatch")]
    WrongMint,
    #[msg("Claimant token account owner mismatch")]
    WrongOwner,
    #[msg("Seed slot must be in the future (commit before the seed is known)")]
    SeedSlotNotFuture,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::str::FromStr;

    fn leaf(index: u64, w: &str, amount: u64) -> [u8; 32] {
        let pk = Pubkey::from_str(w).unwrap();
        hashv(&[
            &[0u8][..],
            &index.to_le_bytes()[..],
            pk.as_ref(),
            &amount.to_le_bytes()[..],
        ])
        .to_bytes()
    }

    fn node(a: [u8; 32], b: [u8; 32]) -> [u8; 32] {
        if a <= b {
            hashv(&[&[1u8][..], &a[..], &b[..]]).to_bytes()
        } else {
            hashv(&[&[1u8][..], &b[..], &a[..]]).to_bytes()
        }
    }

    fn hex(b: &[u8; 32]) -> String {
        b.iter().map(|x| format!("{:02x}", x)).collect()
    }

    /// Vectors generated by the TypeScript `claim-tree.ts` — proves the on-chain
    /// hashing is byte-for-byte identical to the off-chain tree builder.
    #[test]
    fn matches_ts_vectors() {
        let l0 = leaf(0, "11111111111111111111111111111112", 1000);
        let l1 = leaf(1, "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", 2500);
        assert_eq!(
            hex(&l0),
            "d52dc2afb7a3aca337a8276b22511987a55b121cbccd57c2f9a947b5603c1892"
        );
        assert_eq!(
            hex(&l1),
            "dc932a0d47a0d94c85de3fc4b7b3f7a6cf434dce3c563ca5ba12fbeb525fbf63"
        );
        assert_eq!(
            hex(&node(l0, l1)),
            "6b7abd9ce18d3655bf3ea02d10c0557ee7abe1db6a7192aaa631ab21c9d1fc72"
        );
    }
}

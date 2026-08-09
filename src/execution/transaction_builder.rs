use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::{Transaction, VersionedTransaction},
};

/// Builds transactions that are actually submittable.
///
/// The two things that made the previous implementation un-submittable were a
/// default (zero) blockhash and signing with a throwaway keypair. Both are
/// fixed here: the blockhash is fetched from the cluster at build time, and the
/// signer is the wallet the bot actually holds funds in.
pub struct TransactionBuilder {
    wallet: Keypair,
    priority_fee_microlamports: u64,
    /// Compute budget requested for a composed multi-leg transaction.
    ///
    /// The priority fee is charged on the *requested* limit, not the amount
    /// consumed, so over-requesting is a direct cash cost.
    compute_unit_limit: u32,
}

/// Compute budget for a two-leg atomic swap. Enough headroom for both legs
/// plus setup, without over-requesting and paying for unused units.
pub const DEFAULT_COMPOSED_COMPUTE_UNITS: u32 = 600_000;

impl TransactionBuilder {
    pub fn new(wallet: Keypair, priority_fee_microlamports: u64) -> Self {
        Self {
            wallet,
            priority_fee_microlamports,
            compute_unit_limit: DEFAULT_COMPOSED_COMPUTE_UNITS,
        }
    }

    pub fn with_compute_unit_limit(mut self, limit: u32) -> Self {
        self.compute_unit_limit = limit;
        self
    }

    pub fn compute_unit_limit(&self) -> u32 {
        self.compute_unit_limit
    }

    pub fn pubkey(&self) -> Pubkey {
        self.wallet.pubkey()
    }

    /// Decode an aggregator-provided swap transaction and sign it as the fee
    /// payer.
    ///
    /// The aggregator has already chosen the route, accounts and compute
    /// budget, so this must not rebuild the message — only sign it. Re-signing
    /// a fetched message is the whole reason the swap path works without
    /// hand-encoding DEX instructions.
    pub fn sign_encoded_swap(&self, encoded: &str) -> Result<VersionedTransaction> {
        let bytes = BASE64
            .decode(encoded)
            .context("base64-decoding swap transaction")?;
        let tx: VersionedTransaction =
            bincode::deserialize(&bytes).context("deserialising swap transaction")?;

        // Re-sign the message as-is; never mutate it.
        let signed = VersionedTransaction::try_new(tx.message, &[&self.wallet])
            .map_err(|e| anyhow!("signing swap transaction: {}", e))?;
        Ok(signed)
    }

    /// Compose every leg of a route into one atomic transaction.
    ///
    /// This is what makes the round trip real: both swaps land together or
    /// neither does. Returns `Err` when they will not fit, so the caller can
    /// refuse rather than execute a partial trade.
    /// `tip` attaches a Jito tip transfer inside the same transaction, so it is
    /// paid only if the round trip succeeds.
    pub fn compose_legs(
        &self,
        legs: &[crate::prices::SwapInstructions],
        lookup_tables: &[solana_sdk::address_lookup_table_account::AddressLookupTableAccount],
        recent_blockhash: solana_sdk::hash::Hash,
        tip: Option<(Pubkey, u64)>,
    ) -> Result<VersionedTransaction> {
        super::compose::compose_atomic_transaction(
            legs,
            &self.wallet,
            lookup_tables,
            recent_blockhash,
            self.compute_unit_limit,
            self.priority_fee_microlamports,
            tip,
        )
    }

    /// A real but trivial transaction used by `rehearse` mode.
    ///
    /// A zero-lamport self-transfer carrying the same compute-budget and
    /// priority-fee instructions a real trade would. It exercises the entire
    /// sign -> submit -> confirm path against a live cluster without needing
    /// any swap route to exist.
    pub fn build_rehearsal_transaction(&self, rpc: &RpcClient) -> Result<Transaction> {
        let blockhash = rpc
            .get_latest_blockhash()
            .context("fetching recent blockhash for rehearsal transaction")?;

        let me = self.wallet.pubkey();
        let instructions = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(20_000),
            ComputeBudgetInstruction::set_compute_unit_price(self.priority_fee_microlamports),
            system_instruction::transfer(&me, &me, 0),
        ];

        Ok(Transaction::new_signed_with_payer(
            &instructions,
            Some(&me),
            &[&self.wallet],
            blockhash,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage_swap_payload() {
        let builder = TransactionBuilder::new(Keypair::new(), 1_000);
        assert!(builder.sign_encoded_swap("not-valid-base64!!!").is_err());
        // Valid base64 but not a transaction.
        assert!(builder.sign_encoded_swap("aGVsbG8gd29ybGQ=").is_err());
    }

    #[test]
    fn exposes_wallet_pubkey() {
        let kp = Keypair::new();
        let expected = kp.pubkey();
        let builder = TransactionBuilder::new(kp, 1_000);
        assert_eq!(builder.pubkey(), expected);
    }
}

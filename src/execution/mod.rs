pub mod transaction_builder;
pub mod mev_builder;

use crate::types::Route;
use anyhow::Result;
use solana_sdk::transaction::Transaction as SolanaTransaction;
use transaction_builder::TransactionBuilder;
use mev_builder::MEVBuilder;

pub struct ExecutionEngine {
    transaction_builder: TransactionBuilder,
    mev_builder: Option<MEVBuilder>,
}

impl ExecutionEngine {
    pub fn new(transaction_builder: TransactionBuilder, mev_builder: Option<MEVBuilder>) -> Self {
        Self {
            transaction_builder,
            mev_builder,
        }
    }

    pub async fn execute_route(&self, route: Route) -> Result<String> {
        // Build transaction
        let transaction = self.transaction_builder.build_transaction(&route)?;

        // If MEV builder is configured, use it
        if let Some(mev_builder) = &self.mev_builder {
            return mev_builder.submit_bundle(transaction).await;
        }

        // Otherwise submit normally
        self.submit_transaction(transaction).await
    }

    async fn submit_transaction(&self, _transaction: SolanaTransaction) -> Result<String> {
        // Implement transaction submission logic
        Ok("transaction_signature".to_string())
    }
} 
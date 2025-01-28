use anyhow::Result;
use crate::types::Route;

pub mod transaction_builder;
pub mod mev_builder;

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

    pub async fn execute_route(&self, route: &Route, dry_run: bool) -> Result<String> {
        if dry_run {
            Ok("Dry run: Transaction would be executed".to_string())
        } else {
            let tx = self.transaction_builder.build_transaction(route)?;
            
            if let Some(mev_builder) = &self.mev_builder {
                mev_builder.submit_bundle(tx.message.serialize()).await?;
            }
            
            Ok("Transaction executed".to_string())
        }
    }
}
use solana_sdk::{
    instruction::Instruction,
    transaction::Transaction,
    pubkey::Pubkey,
    signer::{keypair::Keypair, Signer},
    hash::Hash,
};
use crate::types::Route;
use anyhow::Result;

pub struct TransactionBuilder {
    program_id: Pubkey,
    fee_payer: Keypair,
}

impl TransactionBuilder {
    /// Creates a new TransactionBuilder
    /// fee_payer: The keypair that will pay for transaction fees
    /// This should be loaded from the wallet configuration or environment
    pub fn new(program_id: Pubkey, fee_payer: Keypair) -> Self {
        Self {
            program_id,
            fee_payer,
        }
    }

    pub fn build_transaction(&self, route: &Route) -> Result<Transaction> {
        let mut instructions = Vec::new();

        for step in &route.steps {
            let swap_ix = self.build_swap_instruction(step)?;
            instructions.push(swap_ix);
        }

        // Get a recent blockhash
        let recent_blockhash = Hash::default(); // Replace with actual blockhash fetch

        Ok(Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.fee_payer.pubkey()),
            &[&self.fee_payer],
            recent_blockhash
        ))
    }

    fn build_swap_instruction(&self, _step: &crate::types::SwapStep) -> Result<Instruction> {
        Ok(Instruction::new_with_bytes(
            self.program_id,
            &[],
            vec![]
        ))
    }
}
use anyhow::Result;
use async_trait::async_trait;
use solana_sdk::pubkey::Pubkey;  // Add this import
use crate::types::{PriceData, Route, DEX, SwapStep};

#[async_trait]
pub trait Strategy: Send + Sync {
    fn name(&self) -> &'static str;
    #[allow(async_fn_in_trait)]
    async fn find_opportunities(&self, prices: &[PriceData]) -> Result<Vec<Route>>;
    fn estimate_profit(&self, route: &Route) -> Result<f64>;
}

pub mod two_hop;
pub use two_hop::TwoHopStrategy;

// Remove the duplicate Strategy trait definition that was here

pub fn create_route_from_price(price: &PriceData, is_testing: bool) -> Route {
    // Define a list of DEXs to consider (excluding Lifinity, Jupiter, and Phoenix)
    let dexes = vec![
        DEX::Raydium,
        DEX::Orca,
        DEX::Meteora,
    ];

    // Initialize variables to track the best opportunity
    let mut best_step: Option<SwapStep> = None;
    let mut best_profit = f64::MIN; // Start with the lowest possible profit

    // Iterate over each DEX to find the best opportunity
    for dex in &dexes {
        // Retrieve the price based on whether we are in testing or production mode
        let (amount_out, _simulated_price) = if is_testing {
            // Simulated price retrieval for testing
            (1000.0, 1000.0) // Example values
        } else {
            // Actual price retrieval logic (to be implemented)
            get_price_from_dex(dex, price) // Pass a reference to dex
        };

        let amount_in: u64 = 1000; // Example amount

        // Calculate profit
        let profit = (amount_out - amount_in as f64) / amount_in as f64 * 100.0;

        // Check if this DEX offers a better profit
        if profit > best_profit {
            best_profit = profit;
            best_step = Some(SwapStep {
                dex: dex.clone(), // Clone dex to avoid moving
                token_in: price.token_pair.split('/').next().unwrap().parse::<Pubkey>().unwrap(),
                token_out: price.token_pair.split('/').nth(1).unwrap().parse::<Pubkey>().unwrap(),
                amount_in,
                amount_out: amount_out as u64, // Convert to u64
                minimum_amount_out: 0, // Placeholder
            });
        }
    }

    // Create the route with the best step found
    let steps = best_step.map_or(vec![], |step| vec![step]);

    Route {
        steps,
        expected_profit: best_profit, // Set the expected profit based on the best opportunity
    }
}

// Placeholder function for getting price from DEX
fn get_price_from_dex(_dex: &DEX, _price: &PriceData) -> (f64, f64) {
    (1000.0, 1000.0)
}
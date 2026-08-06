use anyhow::{anyhow, Context, Result};
use solana_client::rpc_client::RpcClient;
use solana_sdk::transaction::VersionedTransaction;

pub mod compose;
pub mod ledger;
pub mod mev_builder;
pub mod transaction_builder;

use crate::config::{lamports_to_sol, ExecutionMode};
use crate::prices::PriceSource;
use crate::types::Route;
use ledger::TradeLedger;
use transaction_builder::TransactionBuilder;

/// Why a route was not submitted. Every refusal is explicit and named, so a
/// blocked trade can never be mistaken for a successful one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Mode does not submit at all.
    ModeDoesNotSubmit(String),
    /// Live trading requires a per-trade cap to be configured.
    NoSpendCapConfigured,
    /// Live trading requires a cumulative loss cap to be configured.
    NoLossCapConfigured,
    /// Route commits more than the per-trade cap allows.
    ExceedsSpendCap { amount: u64, cap: u64 },
    /// Realised losses have reached the configured ceiling.
    LossCapReached { lost: u64, cap: u64 },
    /// Simulation against the cluster failed.
    SimulationFailed(String),
    /// The legs could not be combined into one atomic transaction, so the
    /// round trip cannot be guaranteed.
    NotAtomic(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::ModeDoesNotSubmit(m) => {
                write!(f, "mode '{}' does not submit transactions", m)
            }
            Refusal::NoSpendCapConfigured => write!(
                f,
                "refusing to trade live: set MAX_TRADE_AMOUNT_SOL to cap each trade"
            ),
            Refusal::NoLossCapConfigured => write!(
                f,
                "refusing to trade live: set MAX_CUMULATIVE_LOSS_SOL to cap total losses"
            ),
            Refusal::ExceedsSpendCap { amount, cap } => write!(
                f,
                "route commits {:.6} SOL, above the {:.6} SOL per-trade cap",
                lamports_to_sol(*amount),
                lamports_to_sol(*cap)
            ),
            Refusal::LossCapReached { lost, cap } => write!(
                f,
                "halted: realised losses {:.6} SOL have reached the {:.6} SOL cap. \
                 Clear data/trades.json to reset once you have reviewed why.",
                lamports_to_sol(*lost),
                lamports_to_sol(*cap)
            ),
            Refusal::SimulationFailed(e) => write!(f, "simulation failed: {}", e),
            Refusal::NotAtomic(e) => write!(
                f,
                "cannot execute atomically, so refusing rather than risk a partial fill: {}",
                e
            ),
        }
    }
}

#[derive(Debug)]
pub enum ExecutionOutcome {
    /// Evaluated only; nothing was built or sent.
    Detected,
    /// Built and simulated successfully, deliberately not submitted.
    Simulated,
    /// Actually submitted and confirmed.
    Submitted { signature: String },
    /// Deliberately not submitted, with a named reason.
    Refused(Refusal),
}

pub struct SafetyLimits {
    pub max_spend_lamports: Option<u64>,
    pub max_cumulative_loss_lamports: Option<u64>,
}

impl SafetyLimits {
    /// Capture the limits in force, for the trade log.
    pub fn snapshot(&self, cumulative_loss: u64) -> crate::types::CapSnapshot {
        crate::types::CapSnapshot {
            max_spend_lamports: self.max_spend_lamports,
            max_cumulative_loss_lamports: self.max_cumulative_loss_lamports,
            cumulative_loss_at_evaluation: cumulative_loss,
        }
    }
}

pub struct ExecutionEngine {
    transaction_builder: TransactionBuilder,
    limits: SafetyLimits,
}

impl ExecutionEngine {
    pub fn new(transaction_builder: TransactionBuilder, limits: SafetyLimits) -> Self {
        Self {
            transaction_builder,
            limits,
        }
    }

    pub fn builder(&self) -> &TransactionBuilder {
        &self.transaction_builder
    }

    pub fn limits(&self) -> &SafetyLimits {
        &self.limits
    }

    /// Check every rule that can block a submission, without doing any I/O.
    ///
    /// Separated from execution so the rules are directly testable and so an
    /// unsafe configuration is caught before a transaction is ever built.
    pub fn check_limits(
        &self,
        mode: ExecutionMode,
        amount_in: u64,
        cumulative_loss: u64,
    ) -> Option<Refusal> {
        if !mode.submits() {
            return Some(Refusal::ModeDoesNotSubmit(mode.to_string()));
        }

        // Caps govern real money only; rehearsal spends nothing but test SOL.
        if !mode.risks_real_funds() {
            return None;
        }

        let spend_cap = match self.limits.max_spend_lamports {
            Some(c) => c,
            None => return Some(Refusal::NoSpendCapConfigured),
        };
        let loss_cap = match self.limits.max_cumulative_loss_lamports {
            Some(c) => c,
            None => return Some(Refusal::NoLossCapConfigured),
        };

        if amount_in > spend_cap {
            return Some(Refusal::ExceedsSpendCap {
                amount: amount_in,
                cap: spend_cap,
            });
        }
        if cumulative_loss >= loss_cap {
            return Some(Refusal::LossCapReached {
                lost: cumulative_loss,
                cap: loss_cap,
            });
        }
        None
    }

    /// Execute a route as far as `mode` allows.
    ///
    /// Order matters and is deliberate: limits are checked before anything is
    /// built, and simulation always runs before submission — including on the
    /// live path, so a route that would fail on-chain never gets sent.
    pub async fn execute_route(
        &self,
        route: &Route,
        mode: ExecutionMode,
        rpc: &RpcClient,
        source: &dyn PriceSource,
        ledger: &TradeLedger,
    ) -> Result<ExecutionOutcome> {
        if mode == ExecutionMode::Detect {
            return Ok(ExecutionOutcome::Detected);
        }

        if mode.submits() {
            if let Some(refusal) =
                self.check_limits(mode, route.amount_in, ledger.cumulative_loss_lamports())
            {
                return Ok(ExecutionOutcome::Refused(refusal));
            }
        }

        // Rehearsal proves the submit path with a trivial transaction, since a
        // mainnet swap cannot execute on devnet.
        if mode == ExecutionMode::Rehearse {
            let tx = self.transaction_builder.build_rehearsal_transaction(rpc)?;
            let sig = rpc
                .send_and_confirm_transaction(&tx)
                .context("submitting rehearsal transaction")?;
            return Ok(ExecutionOutcome::Submitted {
                signature: sig.to_string(),
            });
        }

        // Simulate and live both need the real swap transaction, containing
        // EVERY leg. Executing only the first leg would buy the intermediate
        // token and never sell it, while reporting a round-trip profit that
        // did not happen.
        if route.quotes.is_empty() {
            return Err(anyhow!("route has no quotes; cannot build a swap"));
        }

        let user = self.transaction_builder.pubkey().to_string();
        let mut legs = Vec::with_capacity(route.quotes.len());
        for (i, quote) in route.quotes.iter().enumerate() {
            legs.push(
                source
                    .swap_instructions(quote, &user)
                    .await
                    .with_context(|| format!("fetching swap instructions for leg {}", i + 1))?,
            );
        }

        let table_addresses = compose::merge_lookup_table_addresses(&legs);
        let lookup_tables = compose::load_lookup_tables(rpc, &table_addresses)
            .context("loading address lookup tables")?;

        let blockhash = rpc
            .get_latest_blockhash()
            .context("fetching recent blockhash")?;

        let tx = match self
            .transaction_builder
            .compose_legs(&legs, &lookup_tables, blockhash)
        {
            Ok(tx) => tx,
            // A round trip that cannot be made atomic must not be attempted at
            // all — a partial fill is worse than no trade.
            Err(e) => {
                return Ok(ExecutionOutcome::Refused(Refusal::NotAtomic(e.to_string())))
            }
        };

        // Always simulate first, including on the live path.
        if let Err(e) = simulate(rpc, &tx) {
            return Ok(ExecutionOutcome::Refused(Refusal::SimulationFailed(
                e.to_string(),
            )));
        }

        if mode == ExecutionMode::Simulate {
            return Ok(ExecutionOutcome::Simulated);
        }

        let sig = rpc
            .send_and_confirm_transaction(&tx)
            .context("submitting swap transaction")?;
        Ok(ExecutionOutcome::Submitted {
            signature: sig.to_string(),
        })
    }
}

fn simulate(rpc: &RpcClient, tx: &VersionedTransaction) -> Result<()> {
    let result = rpc
        .simulate_transaction(tx)
        .context("simulating transaction")?;
    if let Some(err) = result.value.err {
        let logs = result
            .value
            .logs
            .map(|l| l.join("\n"))
            .unwrap_or_default();
        return Err(anyhow!("{}{}", err, if logs.is_empty() { String::new() } else { format!("\n{}", logs) }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::Keypair;

    fn engine(spend: Option<u64>, loss: Option<u64>) -> ExecutionEngine {
        ExecutionEngine::new(
            TransactionBuilder::new(Keypair::new(), 1_000),
            SafetyLimits {
                max_spend_lamports: spend,
                max_cumulative_loss_lamports: loss,
            },
        )
    }

    #[test]
    fn non_submitting_modes_are_refused_early() {
        let e = engine(Some(1_000_000), Some(1_000_000));
        assert!(matches!(
            e.check_limits(ExecutionMode::Detect, 1, 0),
            Some(Refusal::ModeDoesNotSubmit(_))
        ));
        assert!(matches!(
            e.check_limits(ExecutionMode::Simulate, 1, 0),
            Some(Refusal::ModeDoesNotSubmit(_))
        ));
    }

    #[test]
    fn rehearse_needs_no_caps() {
        // Rehearsal spends only test SOL, so caps are irrelevant to it.
        let e = engine(None, None);
        assert_eq!(e.check_limits(ExecutionMode::Rehearse, u64::MAX, 0), None);
    }

    #[test]
    fn live_without_caps_is_refused() {
        let e = engine(None, None);
        assert_eq!(
            e.check_limits(ExecutionMode::Live, 1_000, 0),
            Some(Refusal::NoSpendCapConfigured)
        );

        let e = engine(Some(1_000_000), None);
        assert_eq!(
            e.check_limits(ExecutionMode::Live, 1_000, 0),
            Some(Refusal::NoLossCapConfigured)
        );
    }

    #[test]
    fn live_over_spend_cap_is_refused() {
        let e = engine(Some(1_000), Some(1_000_000));
        assert_eq!(
            e.check_limits(ExecutionMode::Live, 1_001, 0),
            Some(Refusal::ExceedsSpendCap {
                amount: 1_001,
                cap: 1_000
            })
        );
        // Exactly at the cap is allowed.
        assert_eq!(e.check_limits(ExecutionMode::Live, 1_000, 0), None);
    }

    #[test]
    fn live_halts_once_loss_cap_reached() {
        let e = engine(Some(1_000_000), Some(10_000));
        assert_eq!(
            e.check_limits(ExecutionMode::Live, 1_000, 10_000),
            Some(Refusal::LossCapReached {
                lost: 10_000,
                cap: 10_000
            })
        );
        // Just under the cap still trades.
        assert_eq!(e.check_limits(ExecutionMode::Live, 1_000, 9_999), None);
    }
}

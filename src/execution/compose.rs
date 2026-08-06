use anyhow::{anyhow, Context, Result};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    address_lookup_table_account::AddressLookupTableAccount,
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signature::Keypair,
    transaction::VersionedTransaction,
};
use std::collections::HashSet;
use std::str::FromStr;

use crate::prices::SwapInstructions;

/// Solana's hard limit on a serialized transaction.
pub const MAX_TRANSACTION_SIZE: usize = 1232;

/// Ceiling on compute units for a single transaction.
pub const MAX_COMPUTE_UNITS: u32 = 1_400_000;

/// Why a set of legs could not be composed into one atomic transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeError {
    /// Serialized transaction exceeds Solana's size limit.
    TooLarge { size: usize, limit: usize },
    /// Requested compute budget exceeds the per-transaction ceiling.
    TooManyComputeUnits { units: u32, limit: u32 },
}

impl std::fmt::Display for ComposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComposeError::TooLarge { size, limit } => write!(
                f,
                "combined transaction is {} bytes, over the {}-byte limit — \
                 the round trip cannot be made atomic at this size",
                size, limit
            ),
            ComposeError::TooManyComputeUnits { units, limit } => write!(
                f,
                "combined transaction requests {} compute units, over the {} limit",
                units, limit
            ),
        }
    }
}

/// Fetch and deserialize the address lookup tables the legs reference.
///
/// Composed transactions routinely exceed the legacy account limit, so the
/// lookup tables Jupiter used are required to compile a v0 message at all.
pub fn load_lookup_tables(
    rpc: &RpcClient,
    addresses: &[String],
) -> Result<Vec<AddressLookupTableAccount>> {
    let mut tables = Vec::new();

    for addr in addresses {
        let key = Pubkey::from_str(addr)
            .with_context(|| format!("parsing lookup table address '{}'", addr))?;
        let account = rpc
            .get_account(&key)
            .with_context(|| format!("fetching lookup table {}", addr))?;

        let table = solana_sdk::address_lookup_table::state::AddressLookupTable::deserialize(
            &account.data,
        )
        .map_err(|e| anyhow!("deserializing lookup table {}: {:?}", addr, e))?;

        tables.push(AddressLookupTableAccount {
            key,
            addresses: table.addresses.to_vec(),
        });
    }

    Ok(tables)
}

/// Merge the lookup-table addresses of several legs, preserving order and
/// dropping duplicates.
///
/// Legs routed through the same venue usually share tables; including one
/// twice wastes scarce transaction bytes.
pub fn merge_lookup_table_addresses(legs: &[SwapInstructions]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for leg in legs {
        for addr in &leg.address_lookup_tables {
            if seen.insert(addr.clone()) {
                out.push(addr.clone());
            }
        }
    }
    out
}

/// Order the instructions of several legs into one atomic sequence.
///
/// Ordering matters and is deliberate:
/// - a single compute-budget block covers the whole transaction, so each leg's
///   own budget instructions are dropped rather than concatenated
/// - every leg's setup runs before any swap, so accounts exist when needed
/// - all cleanup is deferred to the very end, because leg 1's cleanup would
///   otherwise close accounts leg 2 still needs
pub fn order_instructions(
    legs: &[SwapInstructions],
    compute_unit_limit: u32,
    compute_unit_price_micro_lamports: u64,
    tip: Option<(Pubkey, Pubkey, u64)>,
) -> Vec<Instruction> {
    let mut out = Vec::new();

    out.push(ComputeBudgetInstruction::set_compute_unit_limit(
        compute_unit_limit,
    ));
    out.push(ComputeBudgetInstruction::set_compute_unit_price(
        compute_unit_price_micro_lamports,
    ));

    for leg in legs {
        out.extend(leg.setup.iter().cloned());
    }
    for leg in legs {
        out.push(leg.swap.clone());
    }
    for leg in legs {
        out.extend(leg.cleanup.iter().cloned());
    }

    // The tip goes inside the same transaction as the swaps, so it is paid
    // only if the whole round trip succeeds. A tip in a separate transaction
    // could land while the arbitrage failed, turning a free miss into a cost.
    if let Some((from, to, lamports)) = tip {
        out.push(solana_sdk::system_instruction::transfer(&from, &to, lamports));
    }

    out
}

/// Compose several swap legs into one signed, atomic transaction.
///
/// Returns `Err` rather than dropping a leg when the result will not fit: a
/// partially executed round trip leaves the wallet holding the intermediate
/// token with no closing trade, which is worse than not trading at all.
#[allow(clippy::too_many_arguments)]
pub fn compose_atomic_transaction(
    legs: &[SwapInstructions],
    payer: &Keypair,
    lookup_tables: &[AddressLookupTableAccount],
    recent_blockhash: Hash,
    compute_unit_limit: u32,
    compute_unit_price_micro_lamports: u64,
    tip: Option<(Pubkey, u64)>,
) -> Result<VersionedTransaction> {
    use solana_sdk::signer::Signer;

    if compute_unit_limit > MAX_COMPUTE_UNITS {
        return Err(anyhow!(ComposeError::TooManyComputeUnits {
            units: compute_unit_limit,
            limit: MAX_COMPUTE_UNITS,
        }));
    }

    let tip = tip.map(|(to, lamports)| (payer.pubkey(), to, lamports));
    let instructions = order_instructions(
        legs,
        compute_unit_limit,
        compute_unit_price_micro_lamports,
        tip,
    );

    let message = v0::Message::try_compile(
        &payer.pubkey(),
        &instructions,
        lookup_tables,
        recent_blockhash,
    )
    .context("compiling v0 message for composed transaction")?;

    let tx = VersionedTransaction::try_new(VersionedMessage::V0(message), &[payer])
        .map_err(|e| anyhow!("signing composed transaction: {}", e))?;

    // Check the real serialized size, not an estimate — this is the limit that
    // actually rejects the transaction at the node.
    let size = bincode::serialize(&tx)
        .context("serializing composed transaction to measure size")?
        .len();
    if size > MAX_TRANSACTION_SIZE {
        return Err(anyhow!(ComposeError::TooLarge {
            size,
            limit: MAX_TRANSACTION_SIZE,
        }));
    }

    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::instruction::AccountMeta;

    fn ix(tag: u8) -> Instruction {
        Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![AccountMeta::new(Pubkey::new_unique(), false)],
            data: vec![tag],
        }
    }

    fn leg(tag: u8, tables: &[&str]) -> SwapInstructions {
        SwapInstructions {
            compute_budget: vec![ix(200 + tag)],
            setup: vec![ix(10 + tag)],
            swap: ix(tag),
            cleanup: vec![ix(100 + tag)],
            address_lookup_tables: tables.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn setups_precede_swaps_and_cleanup_is_last() {
        let legs = vec![leg(1, &[]), leg(2, &[])];
        let out = order_instructions(&legs, 400_000, 1_000, None);

        let tags: Vec<u8> = out.iter().map(|i| i.data[0]).collect();
        // First two are compute budget (their data is not our tags).
        let body = &tags[2..];

        // Both setups, then both swaps, then both cleanups.
        assert_eq!(body, &[11, 12, 1, 2, 101, 102]);
    }

    #[test]
    fn leg_compute_budgets_are_dropped_not_concatenated() {
        // Two legs would otherwise contribute two budget blocks, and the last
        // one silently wins — capping the combined transaction at one leg's
        // budget.
        let legs = vec![leg(1, &[]), leg(2, &[])];
        let out = order_instructions(&legs, 400_000, 1_000, None);

        let tags: Vec<u8> = out.iter().map(|i| i.data[0]).collect();
        assert!(!tags.contains(&201), "leg 1 budget must not be carried over");
        assert!(!tags.contains(&202), "leg 2 budget must not be carried over");
    }

    #[test]
    fn compute_budget_is_set_once_at_the_front() {
        let legs = vec![leg(1, &[]), leg(2, &[])];
        let out = order_instructions(&legs, 400_000, 1_000, None);

        let budget_program = solana_sdk::compute_budget::id();
        let budget_count = out.iter().filter(|i| i.program_id == budget_program).count();
        assert_eq!(budget_count, 2, "exactly one limit + one price instruction");
        assert_eq!(out[0].program_id, budget_program);
        assert_eq!(out[1].program_id, budget_program);
    }

    #[test]
    fn tip_rides_inside_the_same_transaction_as_the_swaps() {
        // The tip must share the swaps' fate. In a separate transaction it
        // could land while the arbitrage failed, turning a free miss into a
        // paid one — which would forfeit the main reason to use bundles.
        let legs = vec![leg(1, &[]), leg(2, &[])];
        let payer = Pubkey::new_unique();
        let tip_account = Pubkey::new_unique();

        let out = order_instructions(&legs, 400_000, 1_000, Some((payer, tip_account, 12_345)));

        let system = solana_sdk::system_program::id();
        let tip_ix = out
            .iter()
            .find(|i| i.program_id == system)
            .expect("tip transfer must be present");

        assert!(tip_ix.accounts.iter().any(|a| a.pubkey == tip_account));
        // Last, so it only runs once the swaps have succeeded.
        assert_eq!(out.last().unwrap().program_id, system);
    }

    #[test]
    fn no_tip_instruction_when_not_bundling() {
        let legs = vec![leg(1, &[])];
        let out = order_instructions(&legs, 400_000, 1_000, None);
        let system = solana_sdk::system_program::id();
        assert!(
            !out.iter().any(|i| i.program_id == system),
            "a non-bundle submission must not pay a tip"
        );
    }

    #[test]
    fn lookup_tables_are_deduplicated_across_legs() {
        // Legs routed through the same venue share tables; including one twice
        // wastes bytes we cannot spare.
        let legs = vec![leg(1, &["tableA", "tableB"]), leg(2, &["tableB", "tableC"])];
        let merged = merge_lookup_table_addresses(&legs);
        assert_eq!(merged, vec!["tableA", "tableB", "tableC"]);
    }

    #[test]
    fn rejects_compute_budget_over_the_ceiling() {
        let legs = vec![leg(1, &[])];
        let err = compose_atomic_transaction(
            &legs,
            &Keypair::new(),
            &[],
            Hash::default(),
            MAX_COMPUTE_UNITS + 1,
            1_000,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("compute units"), "got: {}", err);
    }

    #[test]
    fn oversized_transaction_is_refused_not_truncated() {
        // Many accounts and no lookup tables: the only correct outcome is a
        // refusal, never a silently dropped leg.
        let big: Vec<SwapInstructions> = (0..30)
            .map(|i| SwapInstructions {
                compute_budget: vec![],
                setup: vec![],
                swap: Instruction {
                    program_id: Pubkey::new_unique(),
                    accounts: (0..8).map(|_| AccountMeta::new(Pubkey::new_unique(), false)).collect(),
                    data: vec![i; 64],
                },
                cleanup: vec![],
                address_lookup_tables: vec![],
            })
            .collect();

        let err = compose_atomic_transaction(
            &big,
            &Keypair::new(),
            &[],
            Hash::default(),
            400_000,
            1_000,
            None,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("bytes") || msg.contains("compiling"),
            "expected a size/compile failure, got: {}",
            msg
        );
    }

    #[test]
    fn a_normal_two_leg_round_trip_composes_and_signs() {
        let legs = vec![leg(1, &[]), leg(2, &[])];
        let tx = compose_atomic_transaction(
            &legs,
            &Keypair::new(),
            &[],
            Hash::default(),
            400_000,
            1_000,
            None,
        )
        .expect("two small legs should compose");

        assert_eq!(tx.signatures.len(), 1, "fee payer signs");
        // Both swaps present in one transaction: that is the atomicity claim.
        assert_eq!(tx.message.instructions().len(), 8);
    }
}

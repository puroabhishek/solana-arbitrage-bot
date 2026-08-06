use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::types::TradeRecord;

/// Persistent record of trade attempts.
///
/// The cumulative loss cap is derived from this file rather than from memory:
/// a cap that resets when the process restarts is not a cap.
pub struct TradeLedger {
    path: PathBuf,
    records: Vec<TradeRecord>,
}

impl TradeLedger {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let records = if path.exists() {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("reading trade ledger {}", path.display()))?;
            if raw.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&raw)
                    .with_context(|| format!("parsing trade ledger {}", path.display()))?
            }
        } else {
            Vec::new()
        };
        Ok(Self { path, records })
    }

    pub fn records(&self) -> &[TradeRecord] {
        &self.records
    }

    pub fn append(&mut self, record: TradeRecord) -> Result<()> {
        self.records.push(record);
        self.flush()
    }

    fn flush(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(&self.records)?;
        fs::write(&self.path, json)
            .with_context(|| format!("writing trade ledger {}", self.path.display()))?;
        Ok(())
    }

    /// Total realised loss in lamports across all recorded trades.
    ///
    /// Only losses count — profits do not top the budget back up. The cap is a
    /// limit on how much this bot may lose while proving itself, not a running
    /// P&L that a lucky streak can reset.
    pub fn cumulative_loss_lamports(&self) -> u64 {
        self.records
            .iter()
            .filter_map(|r| r.realised_profit)
            .filter(|p| *p < 0)
            .map(|p| p.unsigned_abs())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(realised: Option<i64>) -> TradeRecord {
        use crate::strategies::TradeCosts;
        use crate::types::CapSnapshot;

        TradeRecord {
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            mode: "live".to_string(),
            strategy: "two-hop".to_string(),
            label: "SOL/USDC".to_string(),
            legs: Vec::new(),
            amount_in: 1_000_000,
            expected_out: 1_010_000,
            gross_profit: 10_000,
            net_profit: 4_600,
            expected_profit_pct: 1.0,
            costs: TradeCosts::estimate(1, 1_000, 400_000),
            caps: CapSnapshot {
                max_spend_lamports: Some(10_000_000),
                max_cumulative_loss_lamports: Some(50_000_000),
                cumulative_loss_at_evaluation: 0,
            },
            realised_profit: realised,
            signature: None,
            outcome: "confirmed".to_string(),
        }
    }

    #[test]
    fn sums_only_losses() {
        let dir = std::env::temp_dir().join(format!("ledger-test-{}", std::process::id()));
        let path = dir.join("trades.json");
        let mut ledger = TradeLedger::load(&path).unwrap();

        ledger.append(record(Some(-5_000))).unwrap();
        ledger.append(record(Some(20_000))).unwrap(); // profit must not offset
        ledger.append(record(Some(-3_000))).unwrap();
        ledger.append(record(None)).unwrap(); // unknown outcome ignored

        assert_eq!(ledger.cumulative_loss_lamports(), 8_000);

        // Survives a reload — the whole point of persisting it.
        let reloaded = TradeLedger::load(&path).unwrap();
        assert_eq!(reloaded.cumulative_loss_lamports(), 8_000);
        assert_eq!(reloaded.records().len(), 4);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_starts_empty() {
        let path = std::env::temp_dir().join("definitely-does-not-exist-ledger.json");
        fs::remove_file(&path).ok();
        let ledger = TradeLedger::load(&path).unwrap();
        assert_eq!(ledger.cumulative_loss_lamports(), 0);
    }
}

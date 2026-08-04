use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use std::time::Duration;

use super::{Level, Notification, Notifier};

/// Discord embed colours (decimal RGB).
const COLOR_INFO: u32 = 0x5865F2; // blurple
const COLOR_NOTABLE: u32 = 0xFEE75C; // yellow
const COLOR_ALERT: u32 = 0xED4245; // red

/// Discord truncates long embeds; keep well inside the limit.
const MAX_DESCRIPTION: usize = 1_800;

/// Posts alerts to a Discord channel via an incoming webhook.
///
/// A webhook is deliberate rather than a full bot: it needs no token, no
/// OAuth and no persistent gateway connection — just an HTTPS POST — so there
/// is nothing to keep alive and nothing extra to secure.
pub struct DiscordNotifier {
    webhook_url: String,
    client: Client,
}

impl DiscordNotifier {
    pub fn new(webhook_url: impl Into<String>) -> Result<Self> {
        let webhook_url = webhook_url.into();

        // Fail at construction rather than silently never delivering.
        if !webhook_url.starts_with("https://") {
            return Err(anyhow!(
                "DISCORD_WEBHOOK_URL must start with https:// (got '{}')",
                webhook_url.chars().take(24).collect::<String>()
            ));
        }

        let client = Client::builder()
            // Never let a slow chat service stall a trading cycle.
            .timeout(Duration::from_secs(5))
            .build()
            .context("building HTTP client for Discord")?;

        Ok(Self {
            webhook_url,
            client,
        })
    }

    /// Build without the https:// requirement.
    ///
    /// Exists so tests can point the notifier at a local plain-HTTP socket and
    /// assert on the exact bytes sent. Not for production use — a webhook URL
    /// carries a secret token and must never travel unencrypted.
    #[doc(hidden)]
    pub fn new_unchecked(webhook_url: impl Into<String>) -> Result<Self> {
        Ok(Self {
            webhook_url: webhook_url.into(),
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .context("building HTTP client for Discord")?,
        })
    }

    fn color(level: Level) -> u32 {
        match level {
            Level::Info => COLOR_INFO,
            Level::Notable => COLOR_NOTABLE,
            Level::Alert => COLOR_ALERT,
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max.min(s.len());
    // Do not split a UTF-8 character.
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &s[..cut])
}

#[async_trait]
impl Notifier for DiscordNotifier {
    fn name(&self) -> &'static str {
        "discord"
    }

    async fn send(&self, n: &Notification) -> Result<()> {
        let fields: Vec<serde_json::Value> = n
            .fields
            .iter()
            .map(|(name, value)| {
                serde_json::json!({
                    "name": truncate(name, 256),
                    "value": truncate(value, 1024),
                    "inline": true,
                })
            })
            .collect();

        let payload = serde_json::json!({
            "embeds": [{
                "title": truncate(&n.title, 256),
                "description": truncate(&n.body, MAX_DESCRIPTION),
                "color": Self::color(n.level),
                "fields": fields,
                "footer": { "text": "solana-arbitrage-bot" },
            }]
        });

        let resp = self
            .client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await
            .context("posting to Discord webhook")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Discord webhook returned {}: {}",
                status,
                body.chars().take(200).collect::<String>()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_https_webhook() {
        // Catch a misconfigured URL at startup, not on the first alert.
        assert!(DiscordNotifier::new("http://example.com/hook").is_err());
        assert!(DiscordNotifier::new("not-a-url").is_err());
        assert!(DiscordNotifier::new("https://discord.com/api/webhooks/1/abc").is_ok());
    }

    #[test]
    fn truncation_is_utf8_safe() {
        // A naive byte slice here would panic mid-character.
        let s = "é".repeat(50);
        let out = truncate(&s, 11);
        assert!(out.ends_with('…'));
        assert!(out.len() <= 14);
    }

    #[test]
    fn short_strings_pass_through_unchanged() {
        assert_eq!(truncate("hello", 100), "hello");
    }

    #[test]
    fn levels_map_to_distinct_colours() {
        assert_ne!(
            DiscordNotifier::color(Level::Info),
            DiscordNotifier::color(Level::Alert)
        );
        assert_eq!(DiscordNotifier::color(Level::Alert), COLOR_ALERT);
    }
}

use anyhow::Result;
use async_trait::async_trait;

pub mod discord;

pub use discord::DiscordNotifier;

/// How urgent an alert is. Maps to colour in rich clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Routine: startup, a detected opportunity.
    Info,
    /// Something was blocked, or a trade landed.
    Notable,
    /// Needs attention: the bot halted, or a submission failed.
    Alert,
}

/// A single alert.
#[derive(Debug, Clone)]
pub struct Notification {
    pub level: Level,
    pub title: String,
    pub body: String,
    /// Short label/value pairs rendered as fields where supported.
    pub fields: Vec<(String, String)>,
}

impl Notification {
    pub fn new(level: Level, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            level,
            title: title.into(),
            body: body.into(),
            fields: Vec::new(),
        }
    }

    pub fn field(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.push((name.into(), value.into()));
        self
    }
}

/// Somewhere alerts can be sent.
#[async_trait]
pub trait Notifier: Send + Sync {
    fn name(&self) -> &'static str;
    async fn send(&self, n: &Notification) -> Result<()>;
}

/// Sends alerts to every configured destination.
///
/// Notification failures are logged and swallowed: a trading bot must not stop
/// trading, or crash, because a chat service is unreachable.
pub struct Notifiers {
    targets: Vec<Box<dyn Notifier>>,
}

impl Notifiers {
    pub fn new(targets: Vec<Box<dyn Notifier>>) -> Self {
        Self { targets }
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.targets.iter().map(|t| t.name()).collect()
    }

    /// Best-effort delivery. Never returns an error.
    pub async fn notify(&self, n: Notification) {
        for target in &self.targets {
            if let Err(e) = target.send(&n).await {
                log::warn!("{} notification failed: {}", target.name(), e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Failing(Arc<AtomicUsize>);

    #[async_trait]
    impl Notifier for Failing {
        fn name(&self) -> &'static str {
            "failing"
        }
        async fn send(&self, _: &Notification) -> Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(anyhow::anyhow!("service unavailable"))
        }
    }

    #[tokio::test]
    async fn delivery_failure_never_propagates() {
        // The bot must keep trading when the chat service is down.
        let calls = Arc::new(AtomicUsize::new(0));
        let n = Notifiers::new(vec![Box::new(Failing(calls.clone()))]);

        n.notify(Notification::new(Level::Alert, "t", "b")).await;

        assert_eq!(calls.load(Ordering::SeqCst), 1, "should still attempt delivery");
    }

    #[tokio::test]
    async fn empty_notifiers_are_a_no_op() {
        let n = Notifiers::new(vec![]);
        assert!(n.is_empty());
        n.notify(Notification::new(Level::Info, "t", "b")).await;
    }

    #[test]
    fn fields_accumulate() {
        let n = Notification::new(Level::Info, "t", "b")
            .field("a", "1")
            .field("b", "2");
        assert_eq!(n.fields.len(), 2);
    }
}

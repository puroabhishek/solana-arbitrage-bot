use anyhow::Result;
use reqwest::Client;

pub struct MEVBuilder {
    endpoint: String,
    client: Client,
}

impl MEVBuilder {
    pub fn new(endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            client: Client::new(),
        }
    }

    pub async fn submit_bundle(&self, transaction: Vec<u8>) -> Result<String> {
        let response = self.client
            .post(&self.endpoint)
            .json(&transaction)
            .send()
            .await?;

        if response.status().is_success() {
            Ok("Bundle submitted successfully".to_string())
        } else {
            Ok(format!("Bundle submission failed: {}", response.status()))
        }
    }
}
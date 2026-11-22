//! Account-scoped instrument condition snapshots, shared with their strategy runners.

use exness_api::InstrumentCondition;
use std::{collections::HashMap, sync::RwLock};
use tokio::sync::watch;

#[derive(Default)]
pub struct InstrumentConditions {
    accounts: RwLock<HashMap<String, watch::Sender<Option<InstrumentCondition>>>>,
}

impl InstrumentConditions {
    /// Install the REST bootstrap snapshot and return a receiver for this account's runner.
    pub fn register(
        &self,
        account_id: &str,
        initial: InstrumentCondition,
    ) -> watch::Receiver<Option<InstrumentCondition>> {
        let mut accounts = self.accounts.write().expect("instrument condition lock poisoned");
        let sender = accounts
            .entry(account_id.to_owned())
            .or_insert_with(|| watch::channel(Some(initial.clone())).0);
        sender.send_if_modified(|current| {
            let next = Some(initial.clone());
            if *current == next {
                false
            } else {
                *current = next;
                true
            }
        });
        sender.subscribe()
    }

    /// Publish only strategy-relevant instrument condition changes.
    pub fn update(&self, account_id: &str, action: &str, condition: InstrumentCondition) {
        let Ok(accounts) = self.accounts.read() else { return };
        let Some(sender) = accounts.get(account_id) else { return };
        let next = if action == "del" { None } else { Some(condition) };
        sender.send_if_modified(|current| {
            if *current == next {
                false
            } else {
                *current = next;
                true
            }
        });
    }
}

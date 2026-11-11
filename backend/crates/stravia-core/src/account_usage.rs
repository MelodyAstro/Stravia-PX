//! Opt-in, account-scoped site spend estimates. Reservations survive crashes.
//!
//! These are credit-equivalent token estimates, not upstream billing records.
//! The pinned rates deliberately include the highest documented speed/cache
//! premium. An interrupted call keeps its full reservation as uncertain spend.
use anyhow::{Context, ensure};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, str::FromStr};
use stravia_runtime_contract::protocol::ir::Usage;
use stravia_vendor_sdk::{OperationOutput, ProviderSnapshot};

use crate::{
    Gateway,
    plugin::{VendorCallContext, VendorRequest},
    storage::{DynStorage, traits::SettingsStore},
};

pub(crate) const SETTING_PREFIX: &str = "px_account_usage:";
const SCALE: u64 = 1_000_000;
const MAX_PENDING: usize = 128;
pub const RATE_DATE: &str = "2026-11-11";

#[derive(Debug, thiserror::Error)]
#[error("The account site spend limit or estimate prevents this request")]
pub(crate) struct AccountSpendDenied;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AccountBudgetInput {
    /// None disables admission control without erasing accumulated estimates.
    pub limit: Option<String>,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AccountBudgetView {
    pub unit: String,
    pub limit: Option<String>,
    pub spent: String,
    pub reserved: String,
    pub uncertain_spend: String,
    pub pending: usize,
    pub unpriced_requests: u64,
    pub revision: u64,
    pub since: Option<String>,
    pub estimated: bool,
    pub rate_date: &'static str,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Ledger {
    limit: Option<u64>,
    spent: u64,
    uncertain_spend: u64,
    #[serde(default)]
    unpriced_requests: u64,
    reservations: BTreeMap<String, u64>,
    revision: u64,
    #[serde(default)]
    config_revision: u64,
    since: String,
}

impl Ledger {
    fn reserved(&self) -> anyhow::Result<u64> {
        self.reservations.values().try_fold(0u64, |sum, value| {
            sum.checked_add(*value)
                .context("Account reservation overflow")
        })
    }
    fn reserve(&mut self, id: &str, amount: u64) -> anyhow::Result<()> {
        if self.limit.is_none() && self.reservations.len() >= MAX_PENDING {
            // Bound the journal without refunding unresolved calls.
            let unresolved = self.reserved()?;
            self.spent = self
                .spent
                .checked_add(unresolved)
                .context("Account spend overflow")?;
            self.uncertain_spend = self
                .uncertain_spend
                .checked_add(unresolved)
                .context("Account spend overflow")?;
            self.reservations.clear();
        }
        ensure!(
            self.reservations.len() < MAX_PENDING,
            "Account has too many unresolved requests"
        );
        ensure!(
            !self.reservations.contains_key(id),
            "Duplicate account reservation"
        );
        let total = self
            .spent
            .checked_add(self.reserved()?)
            .and_then(|total| total.checked_add(amount))
            .context("Account spend overflow")?;
        ensure!(
            self.limit.is_none_or(|limit| total <= limit),
            "Account site spend estimate would exceed its limit"
        );
        self.reservations.insert(id.into(), amount);
        Ok(())
    }
    fn settle(&mut self, id: &str, amount: Option<u64>) -> anyhow::Result<()> {
        let Some(reserved) = self.reservations.remove(id) else {
            return Ok(());
        };
        let charged = amount.unwrap_or(reserved);
        self.spent = self
            .spent
            .checked_add(charged)
            .context("Account spend overflow")?;
        if amount.is_none() {
            self.uncertain_spend = self
                .uncertain_spend
                .checked_add(charged)
                .context("Account spend overflow")?;
        }
        Ok(())
    }
    fn view(&self, unit: &str) -> anyhow::Result<AccountBudgetView> {
        Ok(AccountBudgetView {
            unit: unit.into(),
            limit: self.limit.map(display),
            spent: display(self.spent),
            reserved: display(self.reserved()?),
            uncertain_spend: display(self.uncertain_spend),
            pending: self.reservations.len(),
            unpriced_requests: self.unpriced_requests,
            revision: self.config_revision,
            since: (!self.since.is_empty()).then(|| self.since.clone()),
            estimated: true,
            rate_date: RATE_DATE,
        })
    }
}

fn display(value: u64) -> String {
    Decimal::from(value)
        .checked_div(Decimal::from(SCALE))
        .expect("nonzero scale")
        .normalize()
        .to_string()
}
fn parse_limit(value: &str) -> anyhow::Result<u64> {
    let number = Decimal::from_str(value).context("Spend limit must be a decimal number")?;
    ensure!(
        !number.is_sign_negative() && number.scale() <= 6,
        "Spend limit must be nonnegative with at most six decimal places"
    );
    number
        .checked_mul(Decimal::from(SCALE))
        .and_then(|n| n.to_u64())
        .context("Spend limit is too large")
}

/// Upstream account identifiers are hashed, never sent to the admin UI.
/// Multiple Providers using the same account share one ledger.
pub(crate) fn account_key(provider: &ProviderSnapshot) -> anyhow::Result<(String, &'static str)> {
    let (field, unit) = match provider.provider_id.as_str() {
        "openai-codex" => ("account_id", "credits"),
        "claude-code" => ("organization_uuid", "USD"),
        _ => anyhow::bail!("Account controls are not supported by this provider"),
    };
    let account = provider
        .credentials
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .context("Verified upstream account identity is unavailable")?;
    let digest = Sha256::digest(format!("{}\0{}", provider.provider_id, account.trim()));
    let digest = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok((format!("{SETTING_PREFIX}{digest}"), unit))
}

async fn load(store: &dyn SettingsStore, key: &str) -> anyhow::Result<(Option<String>, Ledger)> {
    let raw = store.get(key).await?;
    let ledger = raw
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .context("Invalid account spend ledger")?
        .unwrap_or_default();
    Ok((raw, ledger))
}

async fn mutate<T>(
    store: &dyn SettingsStore,
    key: &str,
    mut change: impl FnMut(&mut Ledger) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    for _ in 0..64 {
        let (raw, mut ledger) = load(store, key).await?;
        let result = change(&mut ledger)?;
        ledger.revision = ledger
            .revision
            .checked_add(1)
            .context("Account ledger revision overflow")?;
        let next = serde_json::to_string(&ledger)?;
        if store.compare_exchange(key, raw.as_deref(), &next).await? {
            return Ok(result);
        }
        tokio::task::yield_now().await;
    }
    anyhow::bail!("Account spend ledger is busy; retry later")
}

impl Gateway {
    async fn account_budget_identity(&self, id: &str) -> anyhow::Result<(String, &'static str)> {
        self.admin().get_provider(id).await?;
        let context = VendorCallContext::new(
            Default::default(),
            stravia_runtime_contract::Deadline::fixed(
                std::time::Instant::now() + std::time::Duration::from_secs(45),
            ),
        );
        let prepared = self
            .prepare_vendor_execution(id, None, stravia_vendor_sdk::Operation::Allowance, &context)
            .await?;
        account_key(prepared.provider())
    }
    pub(crate) async fn account_budget(&self, id: &str) -> anyhow::Result<AccountBudgetView> {
        let (key, unit) = self.account_budget_identity(id).await?;
        load(self.storage.settings(), &key).await?.1.view(unit)
    }
    pub(crate) async fn set_account_budget(
        &self,
        id: &str,
        input: AccountBudgetInput,
    ) -> anyhow::Result<AccountBudgetView> {
        let (key, unit) = self.account_budget_identity(id).await?;
        let limit = input.limit.as_deref().map(parse_limit).transpose()?;
        mutate(self.storage.settings(), &key, |ledger| {
            ensure!(
                ledger.config_revision == input.expected_revision,
                "Account spend settings changed; refresh before saving"
            );
            ledger.limit = limit;
            ledger.config_revision = ledger
                .config_revision
                .checked_add(1)
                .context("Account configuration revision overflow")?;
            if ledger.since.is_empty() {
                ledger.since = chrono::Utc::now().to_rfc3339();
            }
            Ok(())
        })
        .await?;
        load(self.storage.settings(), &key).await?.1.view(unit)
    }
}

#[derive(Clone, Copy, Debug)]
struct Rate {
    input: u64,
    output: u64,
}
impl Rate {
    fn amount(self, input: u64, output: u64) -> anyhow::Result<u64> {
        let total = u128::from(input) * u128::from(self.input)
            + u128::from(output) * u128::from(self.output);
        u64::try_from(total.div_ceil(u128::from(SCALE))).context("Account estimate overflow")
    }
    fn usage(self, usage: &Usage) -> anyhow::Result<Option<u64>> {
        if !usage.required_components_known
            || usage
                .server_tool_use
                .as_ref()
                .is_some_and(|t| t.web_search_requests > 0 || t.web_fetch_requests > 0)
        {
            return Ok(None);
        }
        // Cache input counts differ across protocols. Adding all categories
        // avoids undercounting, even when a protocol already includes them.
        let input = u64::from(usage.prompt_tokens)
            + u64::from(usage.cache_read_tokens.unwrap_or(0))
            + u64::from(usage.cache_creation_tokens.unwrap_or(0));
        let output =
            u64::from(usage.completion_tokens).max(u64::from(usage.reasoning_tokens.unwrap_or(0)));
        self.amount(input, output).map(Some)
    }
}

fn official_rate(provider: &ProviderSnapshot) -> anyhow::Result<Rate> {
    let model = provider
        .model
        .as_deref()
        .context("Account estimate requires a model")?;
    let (input, output, multiplier) = match provider.provider_id.as_str() {
        "openai-codex" => {
            let (i, o) = match model {
                "gpt-6-astra" => (250_000_000, 1_250_000_000),
                "gpt-6.1-sol" | "gpt-6-sol" => (50_000_000, 250_000_000),
                "gpt-6.1-terra" => (25_000_000, 125_000_000),
                "gpt-6-luna" => (2_500_000, 12_500_000),
                "gpt-5.6-sol" | "daybreak-blue" => (100_000_000, 500_000_000),
                "gpt-5.6-terra" => (50_000_000, 300_000_000),
                "gpt-5.6-luna" => (5_000_000, 30_000_000),
                "daybreak-red" => (312_500_000, 1_875_000_000),
                "gpt-rosalind-research" => (125_000_000, 625_000_000),
                _ => anyhow::bail!(
                    "No verified official credit rate for this model; update the rate card or disable the site spend limit"
                ),
            };
            (i, o, 6u64) // highest documented Ultrafast purchased-credit multiplier
        }
        "claude-code" => {
            let name = model
                .strip_prefix("claude-")
                .context("Unknown Claude model")?;
            // Model dates are accepted only after an exact known family.
            let family = name
                .rsplit_once('-')
                .filter(|(_, date)| date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()))
                .map_or(name, |(family, _)| family);
            let (i, o) = match family {
                "fable-5-2" | "mythos-5-2" => (8_000_000, 40_000_000),
                "fable-5-1" | "mythos-5-1" | "fable-5" | "mythos-5" => (10_000_000, 50_000_000),
                "opus-5-5" => (4_000_000, 20_000_000),
                "opus-5" | "opus-4-8" | "opus-4-7" | "opus-4-6" | "opus-4-5" => {
                    (5_000_000, 25_000_000)
                }
                "opus-4-1" | "opus-4" => (15_000_000, 75_000_000),
                "sonnet-5-5" | "sonnet-5" => (2_000_000, 10_000_000),
                "sonnet-4-6" | "sonnet-4-5" | "sonnet-4" => (3_000_000, 15_000_000),
                "haiku-5-5" => (500_000, 2_500_000),
                "haiku-4-5" => (1_000_000, 5_000_000),
                _ => anyhow::bail!(
                    "No verified official USD rate for this model; update the rate card or disable the site spend limit"
                ),
            };
            // 2x speed/long-context premium, 2x one-hour cache writes,
            // and 1.1x US inference geography, applied conservatively.
            (i * 44 / 10, o * 22 / 10, 1)
        }
        _ => anyhow::bail!("Account spend estimation is unsupported"),
    };
    Ok(Rate {
        input: input * multiplier,
        output: output * multiplier,
    })
}

pub(crate) struct BudgetPermit {
    storage: DynStorage,
    key: String,
    id: String,
    rate: Rate,
    finished: bool,
}
impl BudgetPermit {
    pub(crate) async fn finish(mut self, output: Option<&OperationOutput>) -> anyhow::Result<()> {
        let usage = match output {
            Some(OperationOutput::Infer(r)) => Some(&r.usage),
            Some(OperationOutput::Compact(r)) => r.usage.as_ref(),
            _ => None,
        };
        let amount = usage
            .map(|usage| self.rate.usage(usage))
            .transpose()?
            .flatten();
        mutate(self.storage.settings(), &self.key, |ledger| {
            ledger.settle(&self.id, amount)
        })
        .await?;
        self.finished = true;
        Ok(())
    }
}
impl Drop for BudgetPermit {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let (storage, key, id) = (self.storage.clone(), self.key.clone(), self.id.clone());
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if let Err(error) =
                    mutate(storage.settings(), &key, |ledger| ledger.settle(&id, None)).await
                {
                    tracing::warn!(%error,"Account spend reservation remains unresolved");
                }
            });
        }
        // Without a runtime the durable reservation remains charged against
        // future admissions. Restart never silently refunds an unknown call.
    }
}

pub(crate) async fn reserve(
    gateway: &Gateway,
    provider: &ProviderSnapshot,
    request: &VendorRequest,
) -> anyhow::Result<Option<BudgetPermit>> {
    if !matches!(request, VendorRequest::Infer(_) | VendorRequest::Compact(_))
        || !matches!(
            provider.provider_id.as_str(),
            "openai-codex" | "claude-code"
        )
    {
        return Ok(None);
    }
    let Ok((key, _)) = account_key(provider) else {
        return Ok(None);
    };
    let (raw, ledger) = load(gateway.storage.settings(), &key).await?;
    if raw.is_none() {
        return Ok(None);
    }
    let rate = match official_rate(provider) {
        Ok(rate) => rate,
        Err(error) if ledger.limit.is_some() => return Err(error),
        Err(_) => {
            record_unpriced(gateway.storage.settings(), &key).await?;
            return Ok(None);
        }
    };
    let context = provider
        .model_metadata
        .as_ref()
        .and_then(|m| m.extensions.get("limit"))
        .and_then(|v| v.get("context"))
        .and_then(serde_json::Value::as_u64)
        .filter(|n| *n > 0 && *n <= 16_777_216);
    let Some(context) = context else {
        ensure!(
            ledger.limit.is_none(),
            "Account spend limit requires a verified model context bound"
        );
        record_unpriced(gateway.storage.settings(), &key).await?;
        return Ok(None);
    };
    // Reserve both maximum input and maximum output. Cache counts can be
    // reported separately, so reserve three input categories as well.
    let amount = rate.amount(context * 3, context)?;
    let id = uuid::Uuid::new_v4().to_string();
    mutate(gateway.storage.settings(), &key, |ledger| {
        ledger.reserve(&id, amount)
    })
    .await?;
    Ok(Some(BudgetPermit {
        storage: gateway.storage.clone(),
        key,
        id,
        rate,
        finished: false,
    }))
}

async fn record_unpriced(store: &dyn SettingsStore, key: &str) -> anyhow::Result<()> {
    mutate(store, key, |ledger| {
        ensure!(
            ledger.limit.is_none(),
            "Account spend limit was enabled while an unpriced request was being admitted"
        );
        ledger.unpriced_requests = ledger
            .unpriced_requests
            .checked_add(1)
            .context("Unpriced account request count overflow")?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests;

//! Explicit account mutations are serialized and keep their id across failures.
use crate::{
    admin::AdminService,
    plugin::{VendorCallContext, VendorRequest},
};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};
use stravia_vendor_sdk::{
    AllowanceAction, AllowanceActionResult, AllowanceRequest, Capability, Operation,
    OperationOutput,
};
use tokio::sync::Mutex;

static ACCOUNT_ACTIONS: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualResetInput {
    pub request_id: String,
    pub confirmed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ManualResetState {
    pub request_id: Option<String>,
    pub result: Option<String>,
    pub pending: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BoundReceipt {
    #[serde(flatten)]
    state: ManualResetState,
    provider_id: String,
    oauth_connection_id: Option<String>,
}

fn receipt_key(
    prepared: &crate::plugin::execution::PreparedVendorExecution,
) -> anyhow::Result<String> {
    let (account, _) = crate::account_usage::account_key(prepared.provider())?;
    Ok(format!("px_reset_receipt:{account}"))
}

impl AdminService {
    pub async fn provider_reset_state(&self, id: &str) -> anyhow::Result<ManualResetState> {
        self.get_provider(id).await?;
        let context = VendorCallContext::new(
            Default::default(),
            stravia_runtime_contract::Deadline::fixed(Instant::now() + Duration::from_secs(45)),
        );
        let prepared = self
            .gw
            .prepare_vendor_execution(id, None, Operation::Allowance, &context)
            .await?;
        self.gw
            .storage
            .settings()
            .get(&receipt_key(&prepared)?)
            .await?
            .as_deref()
            .map(serde_json::from_str::<BoundReceipt>)
            .transpose()
            .context("Invalid reset receipt")
            .map(|state| state.map(|receipt| receipt.state).unwrap_or_default())
    }
    pub async fn redeem_provider_reset(
        &self,
        id: &str,
        input: ManualResetInput,
    ) -> anyhow::Result<AllowanceActionResult> {
        ensure!(
            input.confirmed,
            "Confirm consuming one reset before continuing"
        );
        uuid::Uuid::parse_str(&input.request_id)
            .context("A stable reset request UUID is required")?;
        let admin = self.clone();
        let id = id.to_owned();
        // An admitted mutation finishes even if the browser disconnects.
        tokio::spawn(async move { admin.redeem_reset_inner(&id, input).await })
            .await
            .context("Reset task failed")?
    }
    async fn redeem_reset_inner(
        &self,
        id: &str,
        input: ManualResetInput,
    ) -> anyhow::Result<AllowanceActionResult> {
        let _guard = ACCOUNT_ACTIONS.lock().await;
        let provider = self.get_provider(id).await?;
        let vendor = provider
            .vendor
            .as_deref()
            .context("Provider has no vendor")?;
        let profile = self.gw.vendor_plugins.descriptor(vendor)?;
        ensure!(
            profile.capabilities.contains(&Capability::AllowanceControl),
            "Provider does not support manual reset controls"
        );
        let context = VendorCallContext::new(
            Default::default(),
            stravia_runtime_contract::Deadline::fixed(Instant::now() + Duration::from_secs(45)),
        );
        let prepared = self
            .gw
            .prepare_vendor_execution(id, None, Operation::Allowance, &context)
            .await?;
        let key = receipt_key(&prepared)?;
        let previous_raw = self.gw.storage.settings().get(&key).await?;
        let previous = previous_raw
            .as_deref()
            .map(serde_json::from_str::<BoundReceipt>)
            .transpose()
            .context("Invalid reset receipt")?;
        let previous_state = previous
            .as_ref()
            .map(|receipt| receipt.state.clone())
            .unwrap_or_default();
        if previous_state.request_id.as_deref() == Some(&input.request_id) {
            if let Some(code) = previous_state.result {
                return Ok(AllowanceActionResult {
                    code,
                    request_id: input.request_id,
                });
            }
        } else {
            ensure!(
                !previous_state.pending,
                "Previous reset outcome is unknown; retry its request id"
            );
        }
        if let Some(receipt) = &previous
            && receipt.state.pending
        {
            ensure!(
                receipt.provider_id == id
                    && receipt.oauth_connection_id.as_deref() == prepared.oauth_connection_id(),
                "The account connection changed while the reset outcome was unresolved"
            );
        }
        let mut receipt = BoundReceipt {
            state: ManualResetState {
                request_id: Some(input.request_id.clone()),
                result: None,
                pending: true,
            },
            provider_id: id.into(),
            oauth_connection_id: prepared.oauth_connection_id().map(str::to_owned),
        };
        let pending_raw = serde_json::to_string(&receipt)?;
        ensure!(
            self.gw
                .storage
                .settings()
                .compare_exchange(&key, previous_raw.as_deref(), &pending_raw)
                .await?,
            "Reset state changed; refresh and reuse its request id"
        );
        let output = self
            .gw
            .execute_prepared_vendor(
                prepared,
                VendorRequest::Allowance(AllowanceRequest {
                    model: None,
                    action: Some(AllowanceAction::RedeemReset {
                        request_id: input.request_id.clone(),
                    }),
                }),
                &context,
            )
            .await?;
        let OperationOutput::Allowance(response) = output.output else {
            anyhow::bail!("Invalid account action response");
        };
        let result = response
            .action_result
            .context("Vendor did not acknowledge the reset action")?;
        ensure!(
            result.request_id == input.request_id,
            "Reset response identity mismatch"
        );
        ensure!(
            [
                "reset",
                "nothing_to_reset",
                "no_credit",
                "already_redeemed",
                "already_used",
                "not_limited",
                "cooldown",
                "ineligible",
                "unavailable"
            ]
            .contains(&result.code.as_str()),
            "Unknown reset outcome"
        );
        receipt.state.result = Some(result.code.clone());
        receipt.state.pending = false;
        ensure!(
            self.gw
                .storage
                .settings()
                .compare_exchange(&key, Some(&pending_raw), &serde_json::to_string(&receipt)?)
                .await?,
            "Reset outcome was received but its state changed; retain the request id"
        );
        Ok(result)
    }
}

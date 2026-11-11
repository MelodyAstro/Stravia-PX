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

impl AdminService {
    pub async fn provider_reset_state(&self, id: &str) -> anyhow::Result<ManualResetState> {
        self.get_provider(id).await?;
        self.gw
            .storage
            .settings()
            .get(&format!("px_reset_receipt:{id}"))
            .await?
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .context("Invalid reset receipt")
            .map(|state| state.unwrap_or_default())
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
        let descriptor = self.gw.vendor_plugins.descriptor(vendor)?;
        let profile = descriptor
            .provider(provider.preset_key.as_deref().unwrap_or(vendor))
            .context("Provider profile is missing")?;
        ensure!(
            profile.capabilities.contains(&Capability::AllowanceControl),
            "Provider does not support manual reset controls"
        );
        let previous = self.provider_reset_state(id).await?;
        if previous.request_id.as_deref() == Some(&input.request_id) {
            if let Some(code) = previous.result {
                return Ok(AllowanceActionResult {
                    code,
                    request_id: input.request_id,
                });
            }
        } else {
            ensure!(
                !previous.pending,
                "Previous reset outcome is unknown; retry its request id"
            );
        }
        let key = format!("px_reset_receipt:{id}");
        let mut state = ManualResetState {
            request_id: Some(input.request_id.clone()),
            result: None,
            pending: true,
        };
        self.gw
            .storage
            .settings()
            .set(&key, &serde_json::to_string(&state)?)
            .await?;
        let context = VendorCallContext::new(
            Default::default(),
            stravia_runtime_contract::Deadline::fixed(Instant::now() + Duration::from_secs(45)),
        );
        let prepared = self
            .gw
            .prepare_vendor_execution(id, None, Operation::Allowance, &context)
            .await?;
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
        state.result = Some(result.code.clone());
        state.pending = false;
        self.gw
            .storage
            .settings()
            .set(&key, &serde_json::to_string(&state)?)
            .await?;
        Ok(result)
    }
}

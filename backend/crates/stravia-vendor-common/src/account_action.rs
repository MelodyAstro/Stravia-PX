//! Persist a mutation identity before any irreversible upstream call.
use crate::common::plugin_error;
use serde_json::{Value, json};
use stravia_vendor_sdk::{
    PluginError,
    state::{self, Cell, Scope},
};

pub struct Receipt {
    cell: Cell,
    revision: u64,
    pub request_id: String,
    pub target: Option<String>,
    pub result: Option<String>,
    binding: String,
}

impl Receipt {
    pub fn open(request_id: &str, binding: &str) -> Result<Self, PluginError> {
        let cell = state::open(Scope::Provider, "strinova.manual-reset.v1")?;
        let snapshot = cell.load()?;
        let stored: Value = snapshot
            .payload
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()
            .map_err(|_| invalid("Reset receipt cannot be read"))?
            .unwrap_or(Value::Null);
        if !stored.is_null()
            && stored["request_id"].as_str() != Some(request_id)
            && stored["result"].is_null()
        {
            return Err(invalid(
                "Previous reset outcome is unknown; reuse its request id",
            ));
        }
        let same = stored["request_id"].as_str() == Some(request_id);
        if !stored.is_null()
            && stored["binding"].as_str() != Some(binding)
            && (same || stored["result"].is_null())
        {
            return Err(invalid(
                "The account changed while a reset outcome was unresolved",
            ));
        }
        Ok(Self {
            cell,
            revision: snapshot.revision,
            request_id: request_id.into(),
            target: same
                .then(|| stored["target"].as_str().map(str::to_owned))
                .flatten(),
            result: same
                .then(|| stored["result"].as_str().map(str::to_owned))
                .flatten(),
            binding: binding.into(),
        })
    }
    pub fn record_target(&mut self, target: Option<String>) -> Result<(), PluginError> {
        self.target = target;
        self.save()
    }
    pub fn finish(&mut self, result: &str) -> Result<(), PluginError> {
        self.result = Some(result.into());
        self.save()
    }
    fn save(&mut self) -> Result<(), PluginError> {
        let data = serde_json::to_vec(
            &json!({"request_id":self.request_id,"target":self.target,"result":self.result,"binding":self.binding}),
        )
        .map_err(|_| invalid("Reset receipt cannot be encoded"))?;
        self.cell.compare_exchange(self.revision, Some(&data))?;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("Reset receipt revision overflow"))?;
        Ok(())
    }
}
fn invalid(message: &str) -> PluginError {
    plugin_error(stravia_vendor_sdk::ErrorKind::Invalid, message)
}

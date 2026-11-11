//! Bounded, display-safe account reset inventories. Missing is never zero.
use serde_json::Value;
use stravia_vendor_sdk::{AllowanceAmount, AllowanceItem};

pub fn item(key: &str, count: u64, expires_at: Option<i64>) -> AllowanceItem {
    AllowanceItem {
        key: key.into(),
        label: "Available limit resets".into(),
        kind: "request_allowance".into(),
        used: None,
        remaining: Some(AllowanceAmount {
            value: count.to_string(),
            unit: "resets".into(),
            currency: None,
        }),
        limit: None,
        used_percent: None,
        window_seconds: None,
        resets_at_unix_ms: expires_at,
        condition: Some("unknown".into()),
    }
}

pub fn claude(payload: &Value, now_ms: i64) -> Option<AllowanceItem> {
    let block = payload.get("cedar_ember")?.as_object()?;
    if block.get("eligible")?.as_bool()? != true {
        return None;
    }
    let grants = block.get("grants")?.as_array()?;
    if grants.len() > 200 {
        return None;
    }
    let mut count = 0u64;
    let mut expiry = None;
    for grant in grants {
        let left = grant["resets_left"].as_u64()?;
        let total = grant["resets_total"].as_u64()?;
        let paused = grant["paused"].as_bool()?;
        if left > total {
            return None;
        }
        if paused || left == 0 {
            continue;
        }
        let parse = |key: &str| -> Option<Option<i64>> {
            match grant.get(key)? {
                Value::Null => Some(None),
                value => Some(Some(
                    chrono::DateTime::parse_from_rfc3339(value.as_str()?)
                        .ok()?
                        .timestamp_millis(),
                )),
            }
        };
        let (start, end) = (parse("starts_at")?, parse("ends_at")?);
        if start.is_some_and(|start| start > now_ms) || end.is_some_and(|end| end <= now_ms) {
            continue;
        }
        count = count.checked_add(left)?;
        if count > 50 {
            return None;
        }
        if let Some(end) = end {
            expiry = Some(expiry.map_or(end, |old: i64| old.min(end)));
        }
    }
    Some(item("reset_credits", count, expiry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn expired_paused_future_and_unreadable_grants_never_become_available() {
        let now = 1_800_000_000_000;
        let payload = json!({"cedar_ember":{"eligible":true,"grants":[
            {"id":"PRIVATE-GRANT","resets_left":2,"resets_total":2,"paused":false,"starts_at":null,"ends_at":null},
            {"resets_left":1,"resets_total":1,"paused":true,"starts_at":null,"ends_at":null},
            {"resets_left":1,"resets_total":1,"paused":false,"starts_at":null,"ends_at":"2020-01-01T00:00:00Z"}
        ]}});
        let inventory = claude(&payload, now).unwrap();
        assert_eq!(inventory.remaining.as_ref().unwrap().value, "2");
        assert!(
            !serde_json::to_string(&inventory)
                .unwrap()
                .contains("PRIVATE-GRANT")
        );
        assert!(claude(&json!({}), now).is_none());
        assert!(claude(&json!({"cedar_ember":{"eligible":true,"grants":[{"resets_left":-1,"resets_total":1,"paused":false,"starts_at":null,"ends_at":null}]}}),now).is_none());
        assert!(claude(&json!({"cedar_ember":{"eligible":false,"grants":[]}}), now).is_none());
    }
}

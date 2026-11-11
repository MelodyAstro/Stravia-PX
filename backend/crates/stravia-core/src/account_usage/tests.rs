use super::*;
use crate::storage::{MemoryStorage, SqliteStorage, traits::Storage};
use std::sync::Arc;

fn account_provider(account: &str) -> ProviderSnapshot {
    serde_json::from_value(serde_json::json!({"provider_id":"openai-codex","channel":"codex","base_url":"https://chatgpt.com/backend-api/codex","protocol":"openai-responses","credentials":{"account_id":account},"model":"gpt-6-luna","model_metadata":null})).unwrap()
}

#[test]
fn account_ledgers_follow_upstream_identity_and_unknown_rates_are_never_zero() -> anyhow::Result<()>
{
    let first = account_provider("fixture-account-a");
    let mut duplicate = first.clone();
    duplicate.base_url = "https://alternate.invalid".into();
    assert_eq!(account_key(&first)?, account_key(&duplicate)?);
    assert_ne!(
        account_key(&first)?,
        account_key(&account_provider("fixture-account-b"))?
    );
    assert!(!account_key(&first)?.0.contains("fixture-account-a"));
    assert_eq!(account_key(&first)?.1, "credits");
    assert!(official_rate(&first)?.input > 0);
    duplicate.model = Some("gpt-unknown-next-model".into());
    assert!(official_rate(&duplicate).is_err());
    Ok(())
}

#[tokio::test]
#[ignore = "requires an explicitly configured isolated PostgreSQL DB_URL"]
async fn account_usage_postgres_compare_exchange_and_concurrency() -> anyhow::Result<()> {
    let url = std::env::var("DB_URL").context("isolated PostgreSQL DB_URL is required")?;
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;
    let schema = format!("strinova_account_usage_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await?;
    let result = async {
        let options: sqlx::postgres::PgConnectOptions = url.parse()?;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options.options([("search_path", schema.as_str())]))
            .await?;
        crate::migrations::migrate_postgres(&pool, None).await?;
        let storage = Arc::new(crate::storage::PostgresStorage::from_pool(pool.clone()));
        exercise_cas(storage.settings()).await?;
        mutate(storage.settings(), "budget", |ledger| {
            ledger.limit = Some(10);
            Ok(())
        })
        .await?;
        let mut tasks = Vec::new();
        for index in 0..16 {
            let storage = storage.clone();
            tasks.push(tokio::spawn(async move {
                mutate(storage.settings(), "budget", |ledger| {
                    ledger.reserve(&index.to_string(), 3)
                })
                .await
                .is_ok()
            }));
        }
        let mut admitted = 0;
        for task in tasks {
            admitted += usize::from(task.await?);
        }
        assert_eq!(admitted, 3);
        assert_eq!(load(storage.settings(), "budget").await?.1.reserved()?, 9);
        pool.close().await;
        anyhow::Ok(())
    }
    .await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await?;
    result
}

async fn exercise_cas(store: &dyn SettingsStore) -> anyhow::Result<()> {
    assert!(!store.compare_exchange("absent", Some("old"), "bad").await?);
    assert_eq!(store.get("absent").await?, None);
    assert!(store.compare_exchange("absent", None, "first").await?);
    assert!(!store.compare_exchange("absent", None, "second").await?);
    assert!(
        !store
            .compare_exchange("absent", Some("wrong"), "bad")
            .await?
    );
    assert!(
        store
            .compare_exchange("absent", Some("first"), "next")
            .await?
    );
    assert_eq!(store.get("absent").await?.as_deref(), Some("next"));
    Ok(())
}

#[tokio::test]
async fn settings_compare_exchange_memory_and_sqlite_have_identical_absence_semantics()
-> anyhow::Result<()> {
    exercise_cas(MemoryStorage::new(vec![], vec![], vec![]).settings()).await?;
    let directory = tempfile::tempdir()?;
    let pool = crate::db::init_pool(directory.path()).await?;
    crate::migrations::migrate_sqlite(&pool, None).await?;
    exercise_cas(SqliteStorage::from_pool(pool).settings()).await?;
    Ok(())
}

#[tokio::test]
async fn concurrent_reservations_do_not_overspend_and_restart_does_not_refund() -> anyhow::Result<()>
{
    let directory = tempfile::tempdir()?;
    let pool = crate::db::init_pool(directory.path()).await?;
    crate::migrations::migrate_sqlite(&pool, None).await?;
    let storage = Arc::new(SqliteStorage::from_pool(pool.clone()));
    mutate(storage.settings(), "test-budget", |ledger| {
        ledger.limit = Some(10);
        Ok(())
    })
    .await?;
    let mut tasks = Vec::new();
    for index in 0..16 {
        let storage = storage.clone();
        tasks.push(tokio::spawn(async move {
            mutate(storage.settings(), "test-budget", |ledger| {
                ledger.reserve(&index.to_string(), 3)
            })
            .await
            .is_ok()
        }));
    }
    let mut admitted = 0;
    for task in tasks {
        admitted += usize::from(task.await?);
    }
    assert_eq!(admitted, 3);
    let restarted = SqliteStorage::from_pool(pool);
    let (_, ledger) = load(restarted.settings(), "test-budget").await?;
    assert_eq!(ledger.reserved()?, 9);
    assert!(ledger.spent + ledger.reserved()? <= ledger.limit.unwrap());
    Ok(())
}

#[test]
fn settlement_is_idempotent_and_uncertain_calls_are_never_refunded() -> anyhow::Result<()> {
    let mut ledger = Ledger {
        limit: Some(100),
        ..Default::default()
    };
    ledger.reserve("known", 30)?;
    ledger.reserve("unknown", 40)?;
    ledger.settle("known", Some(12))?;
    ledger.settle("known", Some(12))?;
    ledger.settle("unknown", None)?;
    assert_eq!(ledger.spent, 52);
    assert_eq!(ledger.uncertain_spend, 40);
    assert_eq!(ledger.reserved()?, 0);
    assert!(ledger.reserve("over", 49).is_err());
    ledger.reserve("fits", 48)?;
    Ok(())
}

#[test]
fn decimal_limits_and_token_estimates_round_up_without_float_drift() -> anyhow::Result<()> {
    assert_eq!(parse_limit("0.000001")?, 1);
    for value in ["-1", "NaN", "inf", "0.0000001"] {
        assert!(parse_limit(value).is_err());
    }
    let rate = Rate {
        input: 2_500_000,
        output: 12_500_000,
    };
    assert_eq!(rate.amount(1, 0)?, 3);
    assert_eq!(rate.amount(0, 1)?, 13);
    let usage = Usage {
        prompt_tokens: 10,
        completion_tokens: 5,
        cache_read_tokens: Some(2),
        cache_creation_tokens: Some(3),
        required_components_known: true,
        ..Default::default()
    };
    assert_eq!(rate.usage(&usage)?, Some(100));
    assert!(rate.usage(&Usage::default())?.is_none());
    Ok(())
}

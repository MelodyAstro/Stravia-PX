use super::*;
use serde_json::json;

fn manifest() -> Value {
    json!({"id":"fixture.platform","name":"Runtime fixture","version":"1.0.0","api_version":1,"hooks":["request"],"tools":[{"name":"echo","description":"Echo a JSON message","parameters":{"type":"object"},"parallel_safe":true}]})
}

fn component(manifest: &Value, body: &str) -> Vec<u8> {
    let descriptor = serde_json::to_string(manifest).unwrap();
    let data = descriptor
        .bytes()
        .map(|byte| format!("\\{byte:02x}"))
        .collect::<String>();
    wat::parse_str(format!(
        r#"(component
      (core module $m
        (memory (export "memory") 100)
        (global $heap (mut i32) (i32.const 8192))
        (global $counter (mut i32) (i32.const 0))
        (data (i32.const 64) "{data}")
        (data (i32.const 2048) "not-json")
        (func (export "realloc") (param i32 i32 i32 i32) (result i32)
          (local $ptr i32)
          global.get $heap local.tee $ptr local.get 3 i32.add global.set $heap local.get $ptr)
        (func (export "describe") (result i32)
          i32.const 0 i32.const 0 i32.store
          i32.const 4 i32.const 64 i32.store
          i32.const 8 i32.const {length} i32.store
          i32.const 0)
        (func (export "invoke") (param i32 i32 i32 i32) (result i32)
          {body}))
      (core instance $i (instantiate $m))
      (alias core export $i "memory" (core memory $memory))
      (alias core export $i "realloc" (core func $realloc))
      (func (export "describe") (result (result string (error string)))
        (canon lift (core func $i "describe") (memory $memory) (realloc $realloc)))
      (func (export "invoke") (param "operation" string) (param "input" string)
        (result (result string (error string)))
        (canon lift (core func $i "invoke") (memory $memory) (realloc $realloc))))"#,
        length = descriptor.len()
    ))
    .expect("valid executable platform component")
}

const ECHO: &str = "i32.const 16 i32.const 0 i32.store i32.const 20 local.get 2 i32.store i32.const 24 local.get 3 i32.store i32.const 16";

#[tokio::test]
async fn loads_and_executes_a_real_component() -> Result<()> {
    let runtime = PlatformPluginRuntime::new()?;
    let bytes = component(&manifest(), ECHO);
    let expected: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let loaded = runtime.load(bytes).await?;
    assert_eq!(loaded.manifest().id, "fixture.platform");
    assert_eq!(loaded.sha256(), expected);
    let input = json!({"message":"hello","unicode":"插件"});
    assert_eq!(loaded.invoke("tool:echo", &input).await?, input);
    assert!(loaded.invoke("tool:undeclared", &input).await.is_err());
    Ok(())
}

#[tokio::test]
async fn rejects_invalid_manifests_and_forbidden_imports() -> Result<()> {
    let runtime = PlatformPluginRuntime::new()?;
    let mut descriptor = manifest();
    descriptor["api_version"] = json!(2);
    assert!(runtime.load(component(&descriptor, ECHO)).await.is_err());
    for import in [
        "evil:host/access@1.0.0",
        "wasi:filesystem/types@0.2.0",
        "wasi:sockets/tcp@0.2.0",
    ] {
        let bytes = wat::parse_str(format!("(component (import \"{import}\" (instance)))"))?;
        let error = runtime
            .load(bytes)
            .await
            .err()
            .expect("forbidden import rejected");
        assert!(
            error.to_string().contains("forbidden interface"),
            "{error:#}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn traps_infinite_guest_execution_and_memory_growth() -> Result<()> {
    let runtime = PlatformPluginRuntime::new()?;
    let looping = runtime
        .load(component(
            &manifest(),
            "(loop $forever br $forever) unreachable",
        ))
        .await?;
    assert!(
        tokio::time::timeout(
            Duration::from_secs(10),
            looping.invoke("request", &json!({}))
        )
        .await
        .expect("infinite guest remained interruptible")
        .is_err()
    );
    let growing = runtime
        .load(component(
            &manifest(),
            "i32.const 2048 memory.grow drop i32.const 16",
        ))
        .await?;
    assert!(growing.invoke("request", &json!({})).await.is_err());
    Ok(())
}

#[tokio::test]
async fn rejects_invalid_and_oversized_json_output() -> Result<()> {
    let runtime = PlatformPluginRuntime::new()?;
    for length in [8, MAX_MESSAGE_BYTES + 1] {
        let body = format!(
            "i32.const 16 i32.const 0 i32.store i32.const 20 i32.const 2048 i32.store i32.const 24 i32.const {length} i32.store i32.const 16"
        );
        let loaded = runtime.load(component(&manifest(), &body)).await?;
        assert!(loaded.invoke("request", &json!({})).await.is_err());
    }
    Ok(())
}

#[tokio::test]
async fn every_invocation_gets_fresh_guest_state() -> Result<()> {
    let body = format!(
        "global.get $counter i32.const 0 i32.ne if unreachable end global.get $counter i32.const 1 i32.add global.set $counter {ECHO}"
    );
    let loaded = PlatformPluginRuntime::new()?
        .load(component(&manifest(), &body))
        .await?;
    assert_eq!(
        loaded.invoke("request", &json!({"round":1})).await?,
        json!({"round":1})
    );
    assert_eq!(
        loaded.invoke("request", &json!({"round":2})).await?,
        json!({"round":2})
    );
    Ok(())
}

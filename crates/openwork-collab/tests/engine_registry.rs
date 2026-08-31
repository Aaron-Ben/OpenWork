use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use async_trait::async_trait;
use openwork_collab::computer::{
    engine::{
        AgentEngineRuntime, ClassifyRequest, ClassifyResult, EngineAdapter, EngineAvailability,
        EngineError, EngineId, EngineInventory, EngineRegistry, EngineRuntimeConfig, EngineUsage,
        TurnRequest, TurnResult,
    },
    opencode::OpenCodeAdapter,
};

#[derive(Clone)]
struct FakeEngineAdapter {
    id: EngineId,
}

impl FakeEngineAdapter {
    fn new() -> Self {
        Self {
            id: EngineId::new("fake").unwrap(),
        }
    }
}

struct FakeAgentRuntime {
    agent_id: String,
    turns: u32,
}

#[async_trait]
impl EngineAdapter for FakeEngineAdapter {
    fn id(&self) -> EngineId {
        self.id.clone()
    }

    async fn probe(&self) -> Result<EngineInventory, EngineError> {
        Ok(EngineInventory {
            availability: EngineAvailability::Available,
        })
    }

    async fn classify(&self, request: ClassifyRequest) -> Result<ClassifyResult, EngineError> {
        Ok(ClassifyResult {
            text: request.prompt,
            model: request.model,
            usage: EngineUsage::default(),
        })
    }

    async fn create_agent_runtime(
        &self,
        config: EngineRuntimeConfig,
    ) -> Result<Box<dyn AgentEngineRuntime>, EngineError> {
        Ok(Box::new(FakeAgentRuntime {
            agent_id: config
                .home
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            turns: 0,
        }))
    }
}

#[async_trait]
impl AgentEngineRuntime for FakeAgentRuntime {
    async fn run_turn(&mut self, request: TurnRequest) -> Result<TurnResult, EngineError> {
        self.turns += 1;
        match request.prompt.as_str() {
            "rate-limited" => Err(EngineError::RateLimited {
                retry_after: Some(Duration::from_secs(7)),
                detail: "fake quota".to_string(),
            }),
            "session-invalid" => Err(EngineError::SessionInvalid {
                detail: "fake session".to_string(),
            }),
            "cancelled" => Err(EngineError::Cancelled),
            "output-limit" => Err(EngineError::OutputLimit {
                stream: "stdout",
                limit: 1024,
            }),
            _ => Ok(TurnResult {
                text: format!("{}:{}", self.agent_id, self.turns),
                model: Some("fake/model".to_string()),
                usage: EngineUsage::default(),
            }),
        }
    }

    async fn shutdown(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
}

#[tokio::test]
async fn registry_creates_one_stateful_runtime_per_agent() {
    let mut registry = EngineRegistry::new();
    registry
        .register(OpenCodeAdapter::with_executable("unused-opencode"))
        .unwrap();
    registry.register(FakeEngineAdapter::new()).unwrap();

    assert_eq!(
        registry
            .adapters()
            .into_iter()
            .map(|adapter| adapter.id().to_string())
            .collect::<Vec<_>>(),
        ["fake", "opencode"]
    );

    let adapter = registry.require(&EngineId::new("fake").unwrap()).unwrap();
    let mut first = adapter
        .create_agent_runtime(runtime_config("agent-a"))
        .await
        .unwrap();
    let mut second = adapter
        .create_agent_runtime(runtime_config("agent-b"))
        .await
        .unwrap();

    assert_eq!(run(&mut *first, "success").await.unwrap().text, "agent-a:1");
    assert_eq!(
        run(&mut *second, "success").await.unwrap().text,
        "agent-b:1"
    );
    assert_eq!(run(&mut *first, "success").await.unwrap().text, "agent-a:2");
}

#[tokio::test]
async fn fake_runtime_exposes_engine_neutral_failure_modes() {
    let mut runtime = FakeEngineAdapter::new()
        .create_agent_runtime(runtime_config("agent-a"))
        .await
        .unwrap();

    assert!(matches!(
        run(&mut *runtime, "rate-limited").await,
        Err(EngineError::RateLimited {
            retry_after: Some(delay),
            ..
        }) if delay == Duration::from_secs(7)
    ));
    assert!(matches!(
        run(&mut *runtime, "session-invalid").await,
        Err(EngineError::SessionInvalid { .. })
    ));
    assert!(matches!(
        run(&mut *runtime, "cancelled").await,
        Err(EngineError::Cancelled)
    ));
    assert!(matches!(
        run(&mut *runtime, "output-limit").await,
        Err(EngineError::OutputLimit {
            stream: "stdout",
            limit: 1024,
        })
    ));
}

fn runtime_config(agent_id: &str) -> EngineRuntimeConfig {
    let home = PathBuf::from("/tmp").join(agent_id);
    EngineRuntimeConfig {
        config_root: home.join("engine-config"),
        state_file: home.join("session.json"),
        context_fingerprint: "test-persona".to_string(),
        home,
        model: "fake/model".to_string(),
        environment: BTreeMap::new(),
    }
}

async fn run(
    runtime: &mut dyn AgentEngineRuntime,
    prompt: &str,
) -> Result<TurnResult, EngineError> {
    runtime
        .run_turn(TurnRequest {
            prompt: prompt.to_string(),
            cancellation: tokio_util::sync::CancellationToken::new(),
        })
        .await
}

use std::{io, sync::Arc, time::Duration};

use axum::Router;
use openwork_collab::{
    OpenCodeClient, OpenCodeServer, SpikeResult, collect_until_idle, message_error,
    print_turn_summary, session_id,
};
use rmcp::{
    RoleServer,
    handler::server::wrapper::Parameters,
    schemars,
    service::RequestContext,
    tool, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::Deserialize;
use serde_json::json;
use tokio::{sync::mpsc, task::JoinHandle, time::timeout};

const TOKEN_HEADER: &str = "x-openwork-token";
const TOKEN: &str = "p0-agent-token-4K8D";
const ECHO_INPUT: &str = "P0_MCP_ECHO_2H7X";

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EchoRequest {
    #[schemars(description = "Text returned unchanged by the spike echo tool")]
    text: String,
}

#[derive(Debug, Clone)]
struct ToolInvocation {
    text: String,
    token: Option<String>,
}

#[derive(Clone)]
struct EchoServer {
    invocations: mpsc::UnboundedSender<ToolInvocation>,
}

impl EchoServer {
    fn new(invocations: mpsc::UnboundedSender<ToolInvocation>) -> Self {
        Self { invocations }
    }
}

#[tool_router(server_handler)]
impl EchoServer {
    #[tool(description = "Return the supplied text unchanged. This is the OpenWork P0 echo probe.")]
    fn echo(
        &self,
        Parameters(EchoRequest { text }): Parameters<EchoRequest>,
        context: RequestContext<RoleServer>,
    ) -> String {
        let token = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.headers.get(TOKEN_HEADER))
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let _ = self.invocations.send(ToolInvocation {
            text: text.clone(),
            token,
        });
        text
    }
}

struct McpServerHandle {
    url: String,
    task: JoinHandle<io::Result<()>>,
}

impl Drop for McpServerHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::main]
async fn main() -> SpikeResult<()> {
    let (mcp, mut invocations) = start_mcp_server().await?;
    let home = tempfile::tempdir()?;
    let config = json!({
        "$schema": "https://opencode.ai/config.json",
        "mcp": {
            "openwork": {
                "type": "remote",
                "url": mcp.url,
                "enabled": true,
                "headers": {"X-OpenWork-Token": TOKEN},
                "oauth": false,
                "timeout": 10000
            }
        }
    });
    let config_path = home.path().join("opencode.json");
    tokio::fs::write(&config_path, serde_json::to_vec_pretty(&config)?).await?;
    println!("opencode.config.path={}", config_path.display());
    println!("opencode.config={config}");

    let mut opencode = OpenCodeServer::start().await?;
    let result = run(opencode.base_url(), home.path(), &mut invocations).await;
    opencode.shutdown().await;
    drop(mcp);
    result
}

async fn run(
    base_url: &str,
    home: &std::path::Path,
    invocations: &mut mpsc::UnboundedReceiver<ToolInvocation>,
) -> SpikeResult<()> {
    let client = OpenCodeClient::new(base_url, home);
    let session = client.create_session("OpenWork P0 spike 4").await?;
    println!("session.create.response={session}");
    let id = session_id(&session)?.to_string();
    let mut events = client.event_stream().await?;
    let prompt = format!(
        "Call the openwork_echo tool exactly once with text {ECHO_INPUT}. Do not use bash and do not merely describe the tool. After the tool returns, reply with its result only."
    );
    let accepted = client.prompt_async(&id, &prompt).await?;
    accepted.print("prompt_async");
    if accepted.status != reqwest::StatusCode::NO_CONTENT {
        return Err(message_error("MCP probe prompt_async was not accepted"));
    }

    let turn = collect_until_idle(&mut events, &id, Duration::from_secs(180)).await?;
    print_turn_summary("turn", &turn);
    let invocation = timeout(Duration::from_secs(5), invocations.recv())
        .await
        .map_err(|_| message_error("Agent never called the remote MCP echo tool"))?
        .ok_or_else(|| message_error("MCP invocation channel closed"))?;
    println!("mcp.tool.invocation={invocation:?}");
    let tool_visible = turn.events.iter().any(|event| {
        event
            .pointer("/properties/part/tool")
            .and_then(serde_json::Value::as_str)
            == Some("openwork_echo")
    });
    println!("mcp.tool.visible_in_agent_event={tool_visible}");

    if invocation.text != ECHO_INPUT {
        return Err(message_error(format!(
            "echo tool received wrong input: {invocation:?}"
        )));
    }
    if invocation.token.as_deref() != Some(TOKEN) {
        return Err(message_error(format!(
            "tool call did not carry configured token: {invocation:?}"
        )));
    }
    if !tool_visible {
        return Err(message_error(
            "SSE stream did not identify openwork_echo as the called tool",
        ));
    }
    println!("SPIKE4_RESULT=成立");
    Ok(())
}

async fn start_mcp_server()
-> SpikeResult<(McpServerHandle, mpsc::UnboundedReceiver<ToolInvocation>)> {
    let (invocation_tx, invocation_rx) = mpsc::unbounded_channel();
    let handler = EchoServer::new(invocation_tx);
    let service: StreamableHttpService<EchoServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(handler.clone()),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default(),
        );
    let app = Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move { axum::serve(listener, app).await });
    let handle = McpServerHandle {
        url: format!("http://{address}/mcp"),
        task,
    };
    println!("mcp.server.url={}", handle.url);
    Ok((handle, invocation_rx))
}

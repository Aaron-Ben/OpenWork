use std::sync::{Arc, Mutex};

use openwork_protocol::{
    approval::{ApprovalResolution, ResolveApproval},
    domain::{ApprovalId, TurnId},
};

use crate::{
    ApplicationError, ChatGenerateRequest, ChatGenerateResponse, ChatRuntime,
    RequestCancelRegistry, TurnLiveEvent, TurnLiveEventKind,
};

pub struct TurnApplicationService {
    runtime: ChatRuntime,
    cancels: RequestCancelRegistry,
}

impl TurnApplicationService {
    pub(crate) fn new(runtime: ChatRuntime) -> Self {
        Self {
            runtime,
            cancels: RequestCancelRegistry::default(),
        }
    }

    pub async fn generate_stream(
        &self,
        request: ChatGenerateRequest,
        on_event: impl FnMut(TurnLiveEvent) + Send + 'static,
    ) -> Result<ChatGenerateResponse, ApplicationError> {
        let request_id = request.request_id.clone();
        let session_id = request.session_id.clone();
        let cancel = self.cancels.register(&request_id);
        let on_event = Arc::new(Mutex::new(on_event));
        let runtime_on_event = Arc::clone(&on_event);
        let result = self
            .runtime
            .generate_stream(request, cancel, move |event| {
                let mut on_event = runtime_on_event
                    .lock()
                    .expect("turn application event mutex poisoned");
                on_event(event);
            })
            .await;
        self.cancels.remove(&request_id);
        match result {
            Ok(response) => Ok(response),
            Err(error) => {
                let error = ApplicationError::from(error);
                let mut on_event = on_event
                    .lock()
                    .expect("turn application event mutex poisoned");
                on_event(TurnLiveEvent::new(
                    &request_id,
                    &session_id,
                    TurnLiveEventKind::Error {
                        message: error.message().to_string(),
                    },
                ));
                Err(error)
            }
        }
    }

    pub async fn resolve_approval(
        &self,
        turn_id: String,
        approval_id: String,
        allow: bool,
        on_event: impl FnMut(TurnLiveEvent) + Send + 'static,
    ) -> Result<(), ApplicationError> {
        let resolution = if allow {
            ApprovalResolution::Allow
        } else {
            ApprovalResolution::Deny {
                reason: "denied by user".to_string(),
            }
        };
        let turn_id = TurnId::new(turn_id);
        let command = ResolveApproval {
            turn_id: turn_id.clone(),
            approval_id: ApprovalId::new(approval_id),
            resolution,
        };
        if self.runtime.is_turn_active(&turn_id)? {
            self.runtime.resolve_active_approval(command).await?;
            return Ok(());
        }

        let request_id = turn_id.to_string();
        let cancel = self.cancels.register(&request_id);
        let result = self
            .runtime
            .resume_approval(command, cancel, on_event)
            .await;
        self.cancels.remove(&request_id);
        result.map_err(ApplicationError::from)
    }

    pub fn cancel(&self, request_id: &str) -> bool {
        self.cancels.cancel(request_id)
    }
}

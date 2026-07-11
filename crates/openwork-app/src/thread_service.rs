use openwork_persistence::{
    Session, SessionError, SessionInput, SessionLoadResult, SessionStore, SessionSummary,
};

use crate::ApplicationError;

pub struct ThreadApplicationService {
    store: SessionStore,
}

impl ThreadApplicationService {
    pub(crate) fn new(store: SessionStore) -> Self {
        Self { store }
    }

    pub async fn list(&self) -> Result<Vec<SessionSummary>, ApplicationError> {
        Ok(self.store.list_sessions().await?)
    }

    pub async fn create(&self, input: SessionInput) -> Result<Session, ApplicationError> {
        Ok(self.store.create_session(input).await?)
    }

    pub async fn load(&self, id: &str) -> Result<SessionLoadResult, ApplicationError> {
        let session = self
            .store
            .load_session(id)
            .await?
            .ok_or_else(|| SessionError::NotFound { id: id.to_string() })?;
        let messages = self.store.load_messages(id).await?;
        Ok(SessionLoadResult { session, messages })
    }

    pub async fn delete(&self, id: &str) -> Result<(), ApplicationError> {
        Ok(self.store.delete_session(id).await?)
    }

    pub async fn rename(&self, id: &str, title: &str) -> Result<Session, ApplicationError> {
        Ok(self.store.rename_session(id, title).await?)
    }
}

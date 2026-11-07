use super::*;
use crate::request_models::{RequestModelConfig, RequestModelsView};

impl AdminService {
    pub async fn request_models(&self, key_id: &str) -> anyhow::Result<RequestModelsView> {
        crate::request_models::view(&self.gw, key_id).await
    }

    pub async fn update_request_models(
        &self,
        key_id: &str,
        config: RequestModelConfig,
    ) -> anyhow::Result<RequestModelsView> {
        let _save = crate::rpm::CONFIG_WRITE.lock().await;
        crate::request_models::save(&self.gw, key_id, config).await?;
        self.request_models(key_id).await
    }

    pub async fn capture_request_models(
        &self,
        key_id: &str,
        enabled: bool,
    ) -> anyhow::Result<RequestModelsView> {
        self.get_api_key(key_id).await?;
        crate::request_models::set_capture(&self.gw, key_id, enabled)?;
        self.request_models(key_id).await
    }

    pub async fn clear_request_model_capture(
        &self,
        key_id: &str,
    ) -> anyhow::Result<RequestModelsView> {
        self.get_api_key(key_id).await?;
        self.gw.request_model_capture.clear(key_id);
        self.request_models(key_id).await
    }
}

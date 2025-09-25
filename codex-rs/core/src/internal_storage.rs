use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{io::ErrorKind, path::{Path, PathBuf}};

pub(crate) const INTERNAL_STORAGE_FILE: &str = "internal_storage.json";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct InternalStorage {
    #[serde(skip)]
    storage_path: PathBuf,
    #[serde(default)]
    pub gpt_5_codex_model_prompt_seen: bool,
}

// TODO(jif) generalise all the file writers and build proper async channel inserters.
impl InternalStorage {
    pub fn load(codex_home: &Path) -> Self {
        let storage_path = codex_home.join(INTERNAL_STORAGE_FILE);
        
        let mut storage = std::fs::read_to_string(&storage_path)
            .and_then(|content| serde_json::from_str(&content).map_err(Into::into))
            .unwrap_or_else(|error| {
                match error.downcast_ref::<std::io::Error>() {
                    Some(io_err) if io_err.kind() == ErrorKind::NotFound => {
                        tracing::debug!("internal storage not found at {}; initializing defaults", storage_path.display());
                    }
                    _ => tracing::warn!("failed to load internal storage: {error:?}"),
                }
                Default::default()
            });
        
        storage.storage_path = storage_path;
        storage
    }

    pub async fn persist(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.storage_path.parent() {
            tokio::fs::create_dir_all(parent).await
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }

        let content = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&self.storage_path, content).await
            .with_context(|| format!("failed to write to {}", self.storage_path.display()))
    }
}

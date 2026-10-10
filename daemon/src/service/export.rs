//! Support commands: the diagnostics bundle and the config folder.

use std::path::PathBuf;

use chrono::Utc;
use flow_core::error::FlowError;
use serde_json::json;

use super::DaemonService;

impl DaemonService {
    /// The keymap/config folder, created if missing so a file manager can
    /// open it. `None` when this engine runs on built-in defaults only.
    pub fn reveal_config_dir(&self) -> Result<PathBuf, FlowError> {
        let dir = self
            .keymap_dir
            .clone()
            .ok_or_else(|| FlowError::Internal("no config folder is configured".into()))?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| FlowError::Internal(format!("could not create {}: {e}", dir.display())))?;
        Ok(dir)
    }

    /// Writes a JSON bundle for support: settings, devices (never keys),
    /// link and permission state, diagnostics and the recent log tail.
    /// Returns the file's path.
    pub async fn export_diagnostics(&self) -> Result<PathBuf, FlowError> {
        let dir = self
            .export_dir
            .clone()
            .ok_or_else(|| FlowError::Internal("no export folder is configured".into()))?;
        let now = Utc::now();
        let bundle = json!({
            "flow_version": env!("CARGO_PKG_VERSION"),
            "exported_at": now.to_rfc3339(),
            "host_os": self.state.read().await.devices.values()
                .find(|d| d.id.0 == flow_core::device::LOCAL_DEVICE_ID)
                .map(|d| d.os),
            // Settings hold no secrets (keys live in the identity store).
            "settings": &*self.settings_tx.borrow(),
            "devices": &*self.devices_tx.borrow(),
            "link_state": &*self.link_state_tx.borrow(),
            "permission": &*self.permission_tx.borrow(),
            "diagnostics": self.diagnostics(),
            "log_tail": self.log_tail.as_ref().map(|l| l.recent_lines()).unwrap_or_default(),
        });
        let path = dir.join(format!(
            "flow-diagnostics-{}.json",
            now.format("%Y%m%d-%H%M%S")
        ));
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(&dir)?;
            std::fs::write(&path, serde_json::to_vec_pretty(&bundle)?)
        };
        write().map_err(|e| FlowError::Internal(format!("could not write the bundle: {e}")))?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    #[tokio::test]
    async fn the_bundle_has_state_but_no_keys_and_lands_in_the_export_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open_in_memory().await.expect("db");
        let service = DaemonService::new_seeded_for_test(storage)
            .await
            .with_diagnostics_export(Some(dir.path().to_path_buf()), None);

        let path = service.export_diagnostics().await.expect("export");
        assert!(path.starts_with(dir.path()));
        let bundle: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        for key in [
            "settings",
            "devices",
            "link_state",
            "permission",
            "diagnostics",
            "log_tail",
        ] {
            assert!(bundle.get(key).is_some(), "bundle lacks {key}");
        }
        assert!(!bundle.to_string().contains("public_key"));
    }

    #[tokio::test]
    async fn the_config_dir_is_created_on_reveal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = Storage::open_in_memory().await.expect("db");
        let service = DaemonService::new_with_options(
            storage,
            crate::service::ServiceOptions {
                keymap_dir: Some(dir.path().join("keymaps")),
                ..Default::default()
            },
        )
        .await;
        let path = service.reveal_config_dir().expect("reveal");
        assert!(path.is_dir());
    }
}

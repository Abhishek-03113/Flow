//! Persisted `FlowSettings` (`docs/archive/tasks/daemon-todos.json` task P3). `load()`
//! bootstraps the single `settings` row with [`FlowSettings::defaults`]
//! the first time it's called against an empty table; every later call
//! (including after a restart) returns whatever was last saved.

use flow_core::control::Role;
use flow_core::settings::{FlowSettings, PointerSensitivity};
use flow_core::switch_key::SwitchKeyBinding;
use rusqlite::{OptionalExtension, Row};

use super::Storage;

#[derive(Clone)]
pub struct SettingsRepo {
    storage: Storage,
}

impl SettingsRepo {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    /// Loads the persisted settings, seeding [`FlowSettings::defaults`]
    /// into the table on first run (empty `settings` table).
    pub async fn load(&self) -> FlowSettings {
        self.storage
            .with_connection(|conn| {
                let existing = conn
                    .query_row(
                        "SELECT launch_at_login, show_tray_icon, auto_reconnect,
                                auto_connect_paired_devices, share_keyboard, share_mouse,
                                debug_logging, pointer_sensitivity, switch_key_label,
                                switch_key_tokens, role, return_home_key_label,
                                return_home_key_tokens, edge_switching, edge_switch_delay_ms
                         FROM settings WHERE id = 1",
                        [],
                        row_to_settings,
                    )
                    .optional()
                    .expect("query settings row");

                match existing {
                    Some(settings) => settings,
                    None => {
                        let defaults = FlowSettings::defaults();
                        insert(conn, &defaults);
                        defaults
                    }
                }
            })
            .await
    }

    /// Persists `settings`, replacing whatever was previously saved.
    pub async fn save(&self, settings: FlowSettings) {
        self.storage
            .with_connection(move |conn| insert(conn, &settings))
            .await
    }
}

fn insert(conn: &rusqlite::Connection, settings: &FlowSettings) {
    conn.execute(
        "INSERT INTO settings (
             id, launch_at_login, show_tray_icon, auto_reconnect,
             auto_connect_paired_devices, share_keyboard, share_mouse,
             debug_logging, pointer_sensitivity, switch_key_label, switch_key_tokens,
             role, return_home_key_label, return_home_key_tokens, edge_switching,
             edge_switch_delay_ms
         ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT (id) DO UPDATE SET
             launch_at_login = excluded.launch_at_login,
             show_tray_icon = excluded.show_tray_icon,
             auto_reconnect = excluded.auto_reconnect,
             auto_connect_paired_devices = excluded.auto_connect_paired_devices,
             share_keyboard = excluded.share_keyboard,
             share_mouse = excluded.share_mouse,
             debug_logging = excluded.debug_logging,
             pointer_sensitivity = excluded.pointer_sensitivity,
             switch_key_label = excluded.switch_key_label,
             switch_key_tokens = excluded.switch_key_tokens,
             role = excluded.role,
             return_home_key_label = excluded.return_home_key_label,
             return_home_key_tokens = excluded.return_home_key_tokens,
             edge_switching = excluded.edge_switching,
             edge_switch_delay_ms = excluded.edge_switch_delay_ms",
        rusqlite::params![
            settings.launch_at_login,
            settings.show_tray_icon,
            settings.auto_reconnect,
            settings.auto_connect_paired_devices,
            settings.share_keyboard,
            settings.share_mouse,
            settings.debug_logging,
            pointer_sensitivity_to_str(settings.pointer_sensitivity),
            settings.switch_key.label,
            serde_json::to_string(&settings.switch_key.keys).expect("serialize switch key tokens"),
            role_to_str(settings.role),
            settings.return_home_key.as_ref().map(|k| &k.label),
            settings
                .return_home_key
                .as_ref()
                .map(|k| serde_json::to_string(&k.keys).expect("serialize return home tokens")),
            settings.edge_switching,
            settings.edge_switch_delay_ms,
        ],
    )
    .expect("upsert settings row");
}

fn row_to_settings(row: &Row) -> rusqlite::Result<FlowSettings> {
    let pointer_sensitivity: String = row.get(7)?;
    let switch_key_tokens: String = row.get(9)?;
    Ok(FlowSettings {
        launch_at_login: row.get(0)?,
        show_tray_icon: row.get(1)?,
        auto_reconnect: row.get(2)?,
        auto_connect_paired_devices: row.get(3)?,
        share_keyboard: row.get(4)?,
        share_mouse: row.get(5)?,
        debug_logging: row.get(6)?,
        pointer_sensitivity: pointer_sensitivity_from_str(&pointer_sensitivity),
        switch_key: SwitchKeyBinding {
            label: row.get(8)?,
            keys: serde_json::from_str(&switch_key_tokens).expect("deserialize switch key tokens"),
        },
        role: role_from_str(&row.get::<_, String>(10)?),
        return_home_key: match (
            row.get::<_, Option<String>>(11)?,
            row.get::<_, Option<String>>(12)?,
        ) {
            (Some(label), Some(tokens)) => Some(SwitchKeyBinding {
                label,
                keys: serde_json::from_str(&tokens).expect("deserialize return home tokens"),
            }),
            _ => None,
        },
        edge_switching: row.get(13)?,
        edge_switch_delay_ms: row.get(14)?,
        // Supplied by the engine, never stored.
        keymap_overrides_path: String::new(),
    })
}

fn role_to_str(role: Role) -> &'static str {
    match role {
        Role::Controller => "controller",
        Role::Controlled => "controlled",
    }
}

fn role_from_str(s: &str) -> Role {
    match s {
        "controlled" => Role::Controlled,
        _ => Role::Controller,
    }
}

fn pointer_sensitivity_to_str(sensitivity: PointerSensitivity) -> &'static str {
    match sensitivity {
        PointerSensitivity::Low => "low",
        PointerSensitivity::Normal => "normal",
        PointerSensitivity::High => "high",
    }
}

fn pointer_sensitivity_from_str(s: &str) -> PointerSensitivity {
    match s {
        "low" => PointerSensitivity::Low,
        "high" => PointerSensitivity::High,
        _ => PointerSensitivity::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    #[tokio::test]
    async fn load_on_empty_db_returns_and_persists_defaults() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let repo = SettingsRepo::new(storage);

        let loaded = repo.load().await;
        assert_eq!(loaded, FlowSettings::defaults());

        // A second load reads the now-persisted row, not re-seeding.
        let loaded_again = repo.load().await;
        assert_eq!(loaded_again, FlowSettings::defaults());
    }

    #[tokio::test]
    async fn save_then_load_round_trips_a_changed_value() {
        let storage = Storage::open_in_memory().await.expect("open db");
        let repo = SettingsRepo::new(storage);

        let mut settings = repo.load().await;
        settings.share_mouse = false;
        settings.pointer_sensitivity = PointerSensitivity::High;
        settings.role = Role::Controlled;
        settings.return_home_key = Some(flow_core::switch_key::presets()[1].clone());
        settings.edge_switching = false;
        settings.edge_switch_delay_ms = 250;
        repo.save(settings.clone()).await;

        let reloaded = repo.load().await;
        assert_eq!(reloaded, settings);
        assert!(!reloaded.share_mouse);
        assert_eq!(reloaded.pointer_sensitivity, PointerSensitivity::High);
    }
}

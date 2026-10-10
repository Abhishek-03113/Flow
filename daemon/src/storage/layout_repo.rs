//! The controller's persisted screen arrangement.

use flow_core::control::layout::ScreenLayout;
use rusqlite::OptionalExtension;

use super::{Storage, StorageError};

#[derive(Clone)]
pub struct LayoutRepo {
    storage: Storage,
}

impl LayoutRepo {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    /// `None` until a layout was saved (or when the stored one is unreadable).
    pub async fn load(&self) -> Result<Option<ScreenLayout>, StorageError> {
        self.storage
            .with_connection(|conn| {
                let json: Option<String> = conn
                    .query_row("SELECT layout FROM screen_layout WHERE id = 1", [], |row| {
                        row.get(0)
                    })
                    .optional()?;
                Ok(json.and_then(|json| serde_json::from_str(&json).ok()))
            })
            .await
    }

    pub async fn save(&self, layout: &ScreenLayout) -> Result<(), StorageError> {
        let json = serde_json::to_string(layout)?;
        self.storage
            .with_connection(move |conn| {
                conn.execute(
                    "INSERT INTO screen_layout (id, layout) VALUES (1, ?1)
                     ON CONFLICT (id) DO UPDATE SET layout = excluded.layout",
                    [json],
                )?;
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow_core::device::ScreenSize;

    #[tokio::test]
    async fn a_saved_layout_loads_back_and_an_unsaved_one_is_none() {
        let repo = LayoutRepo::new(Storage::open_in_memory().await.unwrap());
        assert_eq!(repo.load().await.unwrap(), None);

        let layout = ScreenLayout::local_only(ScreenSize {
            width: 800,
            height: 600,
        });
        repo.save(&layout).await.unwrap();
        assert_eq!(repo.load().await.unwrap(), Some(layout));
    }
}

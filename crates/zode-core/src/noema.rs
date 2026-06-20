use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ZodeNoema {
    root: Option<PathBuf>,
}

impl ZodeNoema {
    pub fn disabled() -> Self {
        Self { root: None }
    }

    pub fn from_root(root: impl AsRef<Path>) -> Self {
        Self {
            root: Some(root.as_ref().to_path_buf()),
        }
    }

    pub fn recall_for_turn(
        &self,
        _query: &str,
        _cwd: Option<&Path>,
    ) -> Result<Option<String>, String> {
        let Some(root) = &self.root else {
            return Ok(None);
        };
        if !root.exists() {
            return Ok(None);
        }

        #[cfg(feature = "noema")]
        {
            use noema_core::api::{NoemaEngine, RecallRequest};
            use noema_core::sensitivity::Principal;

            let engine = NoemaEngine::new(root).map_err(|err| err.to_string())?;
            let pack = engine
                .recall(RecallRequest {
                    principal: Principal::personal(
                        std::env::var("USER").unwrap_or_else(|_| "user".to_string()),
                        "zode",
                    ),
                    query: _query.to_string(),
                    cwd: _cwd.map(Path::to_path_buf),
                    budget_tokens: 1200,
                    host: "zode".to_string(),
                })
                .map_err(|err| err.to_string())?;
            return Ok(Some(pack.to_markdown()));
        }

        #[cfg(not(feature = "noema"))]
        {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_noema_returns_empty_pack() {
        let adapter = ZodeNoema::disabled();
        let pack = adapter.recall_for_turn("hello", None).unwrap();
        assert!(pack.is_none());
    }

    #[test]
    fn unavailable_noema_is_non_fatal() {
        let adapter = ZodeNoema::from_root("/path/that/does/not/exist");
        let pack = adapter.recall_for_turn("rust memory", None).unwrap();
        assert!(pack.is_none());
    }
}

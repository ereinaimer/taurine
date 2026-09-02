use crate::engine::catalog::ActiveWindowInfo;
use std::sync::OnceLock;

#[derive(Default)]
pub struct WindowResolver {
    cached: OnceLock<Option<ActiveWindowInfo>>,
}

impl WindowResolver {
    pub fn lazy() -> Self {
        Self {
            cached: OnceLock::new(),
        }
    }

    pub fn from_info(info: Option<ActiveWindowInfo>) -> Self {
        let lock = OnceLock::new();
        let _ = lock.set(info);
        Self { cached: lock }
    }

    pub fn from_static(window: Option<&str>) -> Self {
        let info = window.map(|s| {
            if s.starts_with('{') {
                serde_json::from_str::<ActiveWindowInfo>(s).unwrap_or_else(|_| ActiveWindowInfo {
                    exec_name: Some(s.to_string()),
                    ..Default::default()
                })
            } else {
                ActiveWindowInfo {
                    exec_name: Some(s.to_string()),
                    ..Default::default()
                }
            }
        });
        Self::from_info(info)
    }

    pub fn resolve(
        &self,
        fetcher: impl FnOnce() -> Option<ActiveWindowInfo>,
    ) -> Option<&ActiveWindowInfo> {
        self.cached.get_or_init(fetcher).as_ref()
    }

    #[allow(dead_code)]
    pub fn get_cached(&self) -> Option<&ActiveWindowInfo> {
        self.cached.get().and_then(|o| o.as_ref())
    }
}

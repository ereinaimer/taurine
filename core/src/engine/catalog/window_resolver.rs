use std::sync::OnceLock;

#[derive(Default)]
pub struct WindowResolver {
    cached: OnceLock<Option<String>>,
}

impl WindowResolver {
    pub fn lazy() -> Self {
        Self {
            cached: OnceLock::new(),
        }
    }

    pub fn from_static(window: Option<&str>) -> Self {
        let lock = OnceLock::new();
        let _ = lock.set(window.map(str::to_string));
        Self { cached: lock }
    }

    pub fn resolve(&self, fetcher: impl FnOnce() -> Option<String>) -> Option<&str> {
        self.cached.get_or_init(fetcher).as_deref()
    }

    #[allow(dead_code)]
    pub fn get_cached(&self) -> Option<&str> {
        self.cached.get().and_then(|o| o.as_deref())
    }
}

use dashmap::DashMap;

#[derive(Debug, Clone)]
pub struct UserSettings {
    pub auto_mode: bool,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self { auto_mode: true }
    }
}

pub struct SettingsMap {
    inner: DashMap<i64, UserSettings>,
}

impl SettingsMap {
    pub fn new() -> Self {
        Self {
            inner: DashMap::new(),
        }
    }

    pub fn get(&self, user_id: i64) -> UserSettings {
        self.inner
            .get(&user_id)
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    pub fn set(&self, user_id: i64, settings: UserSettings) {
        self.inner.insert(user_id, settings);
    }

    pub fn set_auto(&self, user_id: i64, enabled: bool) {
        let mut s = self.get(user_id);
        s.auto_mode = enabled;
        self.inner.insert(user_id, s);
    }
}

use std::collections::BTreeMap;
use std::sync::Mutex;

pub const SECRET_KEYS: &[&str] = &[
    "deepseek_api_key",
    "xai_management_key",
    "openai_admin_key",
    "anthropic_admin_key",
    "opencode_api_key",
    "chatgpt_access_token",
];

pub fn is_allowed_secret(name: &str) -> bool {
    SECRET_KEYS.contains(&name)
}

pub fn secret_label(name: &str) -> &'static str {
    match name {
        "deepseek_api_key" => "DeepSeek API キー",
        "xai_management_key" => "xAI Management キー",
        "openai_admin_key" => "OpenAI Admin キー",
        "anthropic_admin_key" => "Anthropic Admin キー (sk-ant-admin)",
        "opencode_api_key" => "OpenCode API キー",
        "chatgpt_access_token" => "ChatGPT アクセストークン（非公式）",
        _ => "不明な項目",
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("{0}")]
    Message(String),
}

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError>;
    fn delete(&self, key: &str) -> Result<(), SecretError>;
}

#[derive(Default)]
pub struct MemorySecretStore {
    values: Mutex<BTreeMap<String, String>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        let guard = self
            .values
            .lock()
            .map_err(|_| SecretError::Message("資格情報ストアが破損しています".into()))?;
        Ok(guard.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        let mut guard = self
            .values
            .lock()
            .map_err(|_| SecretError::Message("資格情報ストアが破損しています".into()))?;
        guard.insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        let mut guard = self
            .values
            .lock()
            .map_err(|_| SecretError::Message("資格情報ストアが破損しています".into()))?;
        guard.remove(key);
        Ok(())
    }
}

pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new() -> Self {
        Self {
            service: "ai-quota".into(),
        }
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        let entry = keyring::Entry::new(&self.service, key)
            .map_err(|err| SecretError::Message(err.to_string()))?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(SecretError::Message(err.to_string())),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(&self.service, key)
            .map_err(|err| SecretError::Message(err.to_string()))?;
        entry
            .set_password(value)
            .map_err(|err| SecretError::Message(err.to_string()))
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(&self.service, key)
            .map_err(|err| SecretError::Message(err.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(SecretError::Message(err.to_string())),
        }
    }
}

pub fn read_secret(
    store: &dyn SecretStore,
    name: &'static str,
) -> Result<Option<String>, crate::model::ProviderError> {
    match store.get(name) {
        Ok(Some(value)) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None),
        Err(err) => Err(crate::model::ProviderError::Unavailable(format!(
            "資格情報ストア: {err}"
        ))),
    }
}

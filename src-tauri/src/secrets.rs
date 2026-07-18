pub const SERVICE: &str = "axiom.desktop";
pub const REFERENCE_PREFIX: &str = "@credential:";

pub fn store(reference: &str, secret: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, reference)
        .map_err(|e| format!("无法访问系统凭据管理器: {e}"))?;
    entry
        .set_password(secret)
        .map_err(|e| format!("无法写入系统凭据管理器: {e}"))
}

pub fn load(reference: &str) -> Result<String, String> {
    let entry = keyring::Entry::new(SERVICE, reference)
        .map_err(|e| format!("无法访问系统凭据管理器: {e}"))?;
    entry
        .get_password()
        .map_err(|_| format!("凭据引用不可用: {reference}"))
}

pub fn delete(reference: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, reference)
        .map_err(|e| format!("无法访问系统凭据管理器: {e}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("无法删除系统凭据: {e}")),
    }
}

pub fn references_in(value: &serde_json::Value) -> Vec<String> {
    value
        .as_object()
        .into_iter()
        .flat_map(|object| object.values())
        .filter_map(serde_json::Value::as_str)
        .filter_map(reference)
        .map(str::to_string)
        .collect()
}

pub fn tagged(reference: &str) -> String {
    format!("{REFERENCE_PREFIX}{reference}")
}

pub fn reference(value: &str) -> Option<&str> {
    value.strip_prefix(REFERENCE_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_reference_round_trip() {
        let tagged_value = tagged("mcp:server:header:authorization");
        assert_eq!(
            reference(&tagged_value),
            Some("mcp:server:header:authorization")
        );
        assert_eq!(reference("Bearer plaintext"), None);
    }
}

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A capsule identifier, e.g. `cap_8f3a2b`.
///
/// Validation is strict: the prefix is required, and the suffix is a
/// non-empty alphanumeric (lowercase + digits) string. This is enough to
/// reject path-traversal and shell-special characters when a `CapsuleId`
/// is used to name a file on disk.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CapsuleId(String);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapsuleIdError {
    #[error("capsule_id must start with `cap_`")]
    MissingPrefix,
    #[error("capsule_id suffix must be 4-32 lowercase alphanumeric characters")]
    BadSuffix,
}

impl CapsuleId {
    pub fn new(raw: impl Into<String>) -> Result<Self, CapsuleIdError> {
        let raw = raw.into();
        let suffix = raw
            .strip_prefix("cap_")
            .ok_or(CapsuleIdError::MissingPrefix)?;
        if suffix.is_empty() || suffix.len() > 32 {
            return Err(CapsuleIdError::BadSuffix);
        }
        if !suffix
            .chars()
            .all(|c| c.is_ascii_digit() || (c.is_ascii_lowercase() && c.is_ascii_alphanumeric()))
        {
            return Err(CapsuleIdError::BadSuffix);
        }
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CapsuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for CapsuleId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CapsuleId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        CapsuleId::new(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapsuleStatus {
    Active,
    Paused,
    Draft,
    Archived,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputationClass {
    A,
    B,
    C,
}

/// Machine-readable manifest written to `.capsule/manifests/{capsule_id}.json`.
/// Spec §2.1. The daemon-managed fields below the split are the mirror of the
/// "Computed Fields" zone in a capsule note's frontmatter: the plugin never
/// writes them, the daemon updates them from its own ledgers, and the plugin
/// reflects them back into the note so the user sees current numbers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub capsule_id: CapsuleId,
    pub schema: String,
    pub status: CapsuleStatus,
    pub floor_price: String,
    pub computation_classes: Vec<ComputationClass>,
    #[serde(default)]
    pub tags: Vec<String>,

    // ─── Daemon-managed (owned by the serving layer, read-only to the plugin) ───
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_cid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earnings_total: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queries_served: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accessed: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_capsule_id() {
        assert!(CapsuleId::new("cap_8f3a2b").is_ok());
        assert!(CapsuleId::new("cap_abc123").is_ok());
    }

    #[test]
    fn rejects_missing_prefix() {
        assert_eq!(
            CapsuleId::new("8f3a2b").unwrap_err(),
            CapsuleIdError::MissingPrefix
        );
    }

    #[test]
    fn rejects_path_traversal() {
        assert_eq!(
            CapsuleId::new("cap_../etc").unwrap_err(),
            CapsuleIdError::BadSuffix
        );
        assert_eq!(
            CapsuleId::new("cap_/absolute").unwrap_err(),
            CapsuleIdError::BadSuffix
        );
    }

    #[test]
    fn rejects_uppercase_and_specials() {
        assert_eq!(
            CapsuleId::new("cap_ABC").unwrap_err(),
            CapsuleIdError::BadSuffix
        );
        assert_eq!(
            CapsuleId::new("cap_a-b").unwrap_err(),
            CapsuleIdError::BadSuffix
        );
    }

    #[test]
    fn rejects_empty_and_oversize() {
        assert!(CapsuleId::new("cap_").is_err());
        let too_long = format!("cap_{}", "a".repeat(33));
        assert!(CapsuleId::new(too_long).is_err());
    }

    #[test]
    fn manifest_roundtrip_json() {
        let m = Manifest {
            capsule_id: CapsuleId::new("cap_8f3a2b").unwrap(),
            schema: "capsule://health.glucose.continuous".into(),
            status: CapsuleStatus::Active,
            floor_price: "0.08 USDC/query".into(),
            computation_classes: vec![ComputationClass::A, ComputationClass::B],
            tags: vec!["glucose".into(), "cgm".into()],
            payload_cid: None,
            earnings_total: None,
            queries_served: None,
            last_accessed: None,
        };
        let json = serde_json::to_string(&m).unwrap();
        let parsed: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.capsule_id, m.capsule_id);
        assert_eq!(parsed.status, m.status);
        assert_eq!(parsed.computation_classes, m.computation_classes);
    }

    /// Slice-1 manifests (no daemon-managed fields at all) must still parse
    /// cleanly after slice 2 added the optional fields. Default → None.
    #[test]
    fn manifest_backward_compat_slice1() {
        let json = r#"{
            "capsule_id": "cap_8f3a2b",
            "schema": "capsule://draft",
            "status": "draft",
            "floor_price": "0.01 USDC/query",
            "computation_classes": ["A"],
            "tags": []
        }"#;
        let parsed: Manifest = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.payload_cid, None);
        assert_eq!(parsed.earnings_total, None);
        assert_eq!(parsed.queries_served, None);
        assert_eq!(parsed.last_accessed, None);
    }

    /// With daemon-managed fields present, they round-trip.
    #[test]
    fn manifest_roundtrip_with_daemon_fields() {
        let m = Manifest {
            capsule_id: CapsuleId::new("cap_8f3a2b").unwrap(),
            schema: "capsule://x".into(),
            status: CapsuleStatus::Active,
            floor_price: "0.01".into(),
            computation_classes: vec![ComputationClass::A],
            tags: vec![],
            payload_cid: Some("bafy...".into()),
            earnings_total: Some("12.34 USDC".into()),
            queries_served: Some(42),
            last_accessed: Some("2026-04-20T12:00:00Z".into()),
        };
        let json = serde_json::to_string(&m).unwrap();
        let parsed: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.payload_cid.as_deref(), Some("bafy..."));
        assert_eq!(parsed.queries_served, Some(42));
    }

    #[test]
    fn manifest_rejects_bad_capsule_id_in_json() {
        let json = r#"{
            "capsule_id": "not-prefixed",
            "schema": "capsule://x",
            "status": "active",
            "floor_price": "0",
            "computation_classes": []
        }"#;
        assert!(serde_json::from_str::<Manifest>(json).is_err());
    }
}

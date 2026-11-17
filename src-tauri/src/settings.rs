//! Key/value app settings.

use crate::error::{AppError, Result};
use crate::pace::PaceSettings;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub pace_warning_threshold_pct: f64,
    pub lumpy_warning_pct: f64,
    pub pending_stale_days: i64,
    /// Optional AI assist; local model server only (never a cloud endpoint)
    pub ai_enabled: bool,
    pub ai_endpoint: String,
}

fn get(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

fn set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn load(conn: &Connection) -> Result<Settings> {
    let num = |key: &str, default: f64| -> Result<f64> {
        Ok(get(conn, key)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(default))
    };
    Ok(Settings {
        pace_warning_threshold_pct: num("pace_warning_threshold_pct", 10.0)?,
        lumpy_warning_pct: num("lumpy_warning_pct", 90.0)?,
        pending_stale_days: num("pending_stale_days", 10.0)? as i64,
        ai_enabled: get(conn, "ai_enabled")?.as_deref() == Some("true"),
        ai_endpoint: get(conn, "ai_endpoint")?.unwrap_or_default(),
    })
}

/// True for endpoints on this machine (localhost / loopback). Cloud endpoints are not allowed.
pub fn is_local_endpoint(endpoint: &str) -> bool {
    let rest = match endpoint.trim().split_once("://") {
        Some(("http" | "https", rest)) => rest,
        _ => return false,
    };
    let host_port = rest.split('/').next().unwrap_or("");
    let host = if let Some(stripped) = host_port.strip_prefix('[') {
        stripped.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

pub fn save(conn: &Connection, s: &Settings) -> Result<Settings> {
    if !(0.0..=100.0).contains(&s.pace_warning_threshold_pct)
        || !(0.0..=100.0).contains(&s.lumpy_warning_pct)
    {
        return Err(AppError::invalid("percentages must be between 0 and 100"));
    }
    if s.pending_stale_days < 1 {
        return Err(AppError::invalid("stale pending days must be at least 1"));
    }
    if !s.ai_endpoint.trim().is_empty() && !is_local_endpoint(&s.ai_endpoint) {
        return Err(AppError::invalid(
            "the AI endpoint must be a local model server (localhost or 127.0.0.1); cloud endpoints are not allowed",
        ));
    }
    if s.ai_enabled && s.ai_endpoint.trim().is_empty() {
        return Err(AppError::invalid(
            "set a local AI endpoint before enabling AI assist",
        ));
    }
    set(
        conn,
        "pace_warning_threshold_pct",
        &s.pace_warning_threshold_pct.to_string(),
    )?;
    set(conn, "lumpy_warning_pct", &s.lumpy_warning_pct.to_string())?;
    set(
        conn,
        "pending_stale_days",
        &s.pending_stale_days.to_string(),
    )?;
    set(
        conn,
        "ai_enabled",
        if s.ai_enabled { "true" } else { "false" },
    )?;
    set(conn, "ai_endpoint", s.ai_endpoint.trim())?;
    load(conn)
}

pub fn pace(conn: &Connection) -> Result<PaceSettings> {
    let s = load(conn)?;
    Ok(PaceSettings {
        warning_threshold_pct: s.pace_warning_threshold_pct,
        lumpy_warning_pct: s.lumpy_warning_pct,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn defaults_and_round_trip() {
        let conn = db::open_in_memory().unwrap();
        let mut s = load(&conn).unwrap();
        assert_eq!(s.pace_warning_threshold_pct, 10.0);
        assert_eq!(s.pending_stale_days, 10);
        assert!(!s.ai_enabled);
        s.ai_endpoint = "http://localhost:8080/v1".into();
        s.ai_enabled = true;
        let saved = save(&conn, &s).unwrap();
        assert!(saved.ai_enabled);
    }

    #[test]
    fn only_local_ai_endpoints() {
        assert!(is_local_endpoint("http://localhost:8080/v1"));
        assert!(is_local_endpoint("http://127.0.0.1:11434"));
        assert!(is_local_endpoint("http://[::1]:8000/v1"));
        assert!(!is_local_endpoint("https://api.example.com/v1"));
        assert!(!is_local_endpoint("http://localhost.example.com"));
        assert!(!is_local_endpoint("localhost:8080"));
        let conn = db::open_in_memory().unwrap();
        let mut s = load(&conn).unwrap();
        s.ai_endpoint = "https://api.example.com".into();
        assert!(save(&conn, &s).is_err());
    }
}

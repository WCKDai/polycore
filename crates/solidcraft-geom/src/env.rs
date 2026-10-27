//! Polykrom's environment variables, read in one place.
//!
//! Every variable is `POLYKROM_<NAME>`. Compatibility shim (see docs/compatibility.md): when it
//! is unset, the same variable under the former prefixes (`SOLIDCRAFT_`, then `SOLVECRAFT_`) is
//! read instead, so scripts and service files written for earlier releases keep working.

use std::ffi::OsString;

/// The current prefix.
pub const PREFIX: &str = "POLYKROM_";
/// The prefix before Polykrom (SolidCraft's), read as the first fallback.
pub const SOLIDCRAFT_PREFIX: &str = "SOLIDCRAFT_";
/// The oldest prefix, read only as the last fallback.
pub const LEGACY_PREFIX: &str = "SOLVECRAFT_";

/// `POLYKROM_<name>`, else `SOLIDCRAFT_<name>`, else the legacy `SOLVECRAFT_<name>`. `name` is the
/// part after the prefix, e.g. `"RECOVERY_DIR"`.
pub fn var_os(name: &str) -> Option<OsString> {
    lookup(name, |k| std::env::var_os(k))
}

/// [`var_os`] as UTF-8; a value that is not valid UTF-8 counts as unset.
pub fn var(name: &str) -> Option<String> {
    var_os(name).and_then(|v| v.into_string().ok())
}

fn lookup(name: &str, get: impl Fn(&str) -> Option<OsString>) -> Option<OsString> {
    get(&format!("{PREFIX}{name}")).or_else(|| get(&format!("{SOLIDCRAFT_PREFIX}{name}"))).or_else(|| get(&format!("{LEGACY_PREFIX}{name}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let m: HashMap<String, OsString> = pairs.iter().map(|(k, v)| ((*k).to_string(), OsString::from(*v))).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn polykrom_wins_and_solidcraft_is_the_first_fallback() {
        let get = env(&[("POLYKROM_CONTROL_PORT", "1"), ("SOLIDCRAFT_CONTROL_PORT", "2"), ("SOLVECRAFT_CONTROL_PORT", "3")]);
        assert_eq!(lookup("CONTROL_PORT", &get), Some(OsString::from("1")));
        let get = env(&[("SOLIDCRAFT_CONTROL_PORT", "2"), ("SOLVECRAFT_CONTROL_PORT", "3")]);
        assert_eq!(lookup("CONTROL_PORT", &get), Some(OsString::from("2")));
    }

    #[test]
    fn new_prefix_wins_over_legacy() {
        let get = env(&[("SOLIDCRAFT_RECOVERY_DIR", "/new"), ("SOLVECRAFT_RECOVERY_DIR", "/old")]);
        assert_eq!(lookup("RECOVERY_DIR", get), Some(OsString::from("/new")));
    }

    #[test]
    fn legacy_prefix_is_the_fallback() {
        let get = env(&[("SOLVECRAFT_CONTROL_PORT", "4000")]);
        assert_eq!(lookup("CONTROL_PORT", get), Some(OsString::from("4000")));
    }

    #[test]
    fn unset_in_both_is_none() {
        assert_eq!(lookup("WAYLAND", env(&[("OTHER", "1")])), None);
    }
}

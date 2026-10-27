//! Validation for bounded multi-agent task packets.

use std::path::Path;

use serde_json::Value;

pub(crate) struct Summary {
    pub id: String,
    pub title: String,
    pub role: String,
    pub status: String,
    pub attempts: u64,
    pub wall_minutes: u64,
    pub tokens: u64,
    pub usd: f64,
}

pub(crate) fn check(path: &str) -> Result<(), String> {
    let file = Path::new(path);
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let task: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    let summary = validate(&task).map_err(|e| format!("{}: {e}", file.display()))?;
    println!(
        "task {} [{} / {}]: {} attempt(s), {} min, {} tokens, ${:.2}",
        summary.id, summary.role, summary.status, summary.attempts, summary.wall_minutes, summary.tokens, summary.usd
    );
    println!("{}", summary.title);
    Ok(())
}

pub(crate) fn validate(task: &Value) -> Result<Summary, String> {
    let object = task.as_object().ok_or("task must be a JSON object")?;
    let string = |name: &str| -> Result<String, String> {
        let value = object.get(name).ok_or_else(|| format!("missing `{name}`"))?;
        value.as_str().map(str::to_string).ok_or_else(|| format!("`{name}` must be a string"))
    };
    let id = string("id")?;
    if !(3..=64).contains(&id.len())
        || !id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        || !id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err("`id` must be 3-64 lowercase letters, digits or dashes".into());
    }
    let title = string("title")?;
    if !(8..=120).contains(&title.len()) {
        return Err("`title` must be 8-120 characters".into());
    }
    let role = string("role")?;
    const ROLES: [&str; 8] = ["coordinator", "explorer", "geometry", "verification", "simulation", "interchange", "application", "reviewer"];
    if !ROLES.contains(&role.as_str()) {
        return Err(format!("unknown role `{role}`"));
    }
    let objective = string("objective")?;
    if objective.len() < 20 {
        return Err("`objective` must describe at least 20 characters".into());
    }
    let status = string("status")?;
    const STATUSES: [&str; 6] = ["planned", "active", "blocked", "review", "done", "cancelled"];
    if !STATUSES.contains(&status.as_str()) {
        return Err(format!("unknown status `{status}`"));
    }
    let risk = string("risk")?;
    const RISKS: [&str; 4] = ["low", "medium", "high", "critical"];
    if !RISKS.contains(&risk.as_str()) {
        return Err(format!("unknown risk `{risk}`"));
    }

    let nonempty_strings = |name: &str, min: usize| -> Result<Vec<String>, String> {
        let array = object.get(name).and_then(Value::as_array).ok_or_else(|| format!("`{name}` must be an array"))?;
        if array.len() < min {
            return Err(format!("`{name}` needs at least {min} item(s)"));
        }
        let mut out = Vec::with_capacity(array.len());
        for (i, item) in array.iter().enumerate() {
            let text = item.as_str().ok_or_else(|| format!("`{name}[{i}]` must be a string"))?;
            if text.trim().is_empty() {
                return Err(format!("`{name}[{i}]` must not be empty"));
            }
            out.push(text.to_string());
        }
        out.sort();
        if out.windows(2).any(|pair| pair.first() == pair.get(1)) {
            return Err(format!("`{name}` contains a duplicate"));
        }
        Ok(out)
    };
    nonempty_strings("owns", 1)?;
    nonempty_strings("forbids", 0)?;
    nonempty_strings("depends_on", 0)?;
    nonempty_strings("acceptance", 1)?;
    nonempty_strings("evidence", 1)?;

    let budget = object.get("budget").and_then(Value::as_object).ok_or("`budget` must be an object")?;
    let integer = |name: &str, min: u64, max: u64| -> Result<u64, String> {
        let value = budget.get(name).and_then(Value::as_u64).ok_or_else(|| format!("`budget.{name}` must be an unsigned integer"))?;
        if !(min..=max).contains(&value) {
            return Err(format!("`budget.{name}` must be between {min} and {max}"));
        }
        Ok(value)
    };
    let attempts = integer("attempts", 1, 20)?;
    let wall_minutes = integer("wall_minutes", 1, 1440)?;
    let tokens = integer("tokens", 0, 100_000_000)?;
    let usd = budget.get("usd").and_then(Value::as_f64).ok_or("`budget.usd` must be a number")?;
    if !usd.is_finite() || !(0.0..=10_000.0).contains(&usd) {
        return Err("`budget.usd` must be finite and between 0 and 10000".into());
    }
    let model_tier = budget.get("model_tier").and_then(Value::as_str).ok_or("`budget.model_tier` must be a string")?;
    const TIERS: [&str; 5] = ["deterministic", "small", "standard", "strong", "reviewer"];
    if !TIERS.contains(&model_tier) {
        return Err(format!("unknown model tier `{model_tier}`"));
    }

    Ok(Summary { id, title, role, status, attempts, wall_minutes, tokens, usd })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn valid() -> Value {
        json!({
            "id": "contract-tests",
            "title": "Test the typed contract",
            "role": "verification",
            "objective": "Verify refusal and evidence semantics are stable.",
            "owns": ["crates/polycore/src/api.rs"],
            "forbids": [],
            "depends_on": [],
            "acceptance": ["Focused tests pass."],
            "evidence": ["cargo xtask ci output."],
            "risk": "medium",
            "budget": {"attempts": 4, "wall_minutes": 60, "tokens": 100000, "usd": 5.0, "model_tier": "standard"},
            "status": "planned"
        })
    }

    #[test]
    fn a_complete_bounded_task_is_valid() {
        let summary = validate(&valid()).unwrap();
        assert_eq!((summary.id.as_str(), summary.attempts, summary.wall_minutes), ("contract-tests", 4, 60));
        assert_eq!(summary.usd, 5.0);
    }

    #[test]
    fn missing_evidence_and_excessive_budget_are_rejected() {
        let mut task = valid();
        task.as_object_mut().unwrap().remove("evidence");
        assert!(validate(&task).is_err());

        let mut task = valid();
        task["budget"]["attempts"] = json!(21);
        assert!(validate(&task).is_err());
        task["budget"]["attempts"] = json!(4);
        task["budget"]["usd"] = json!(10_001.0);
        assert!(validate(&task).is_err());
    }

    #[test]
    fn unknown_roles_and_duplicate_paths_are_rejected() {
        let mut task = valid();
        task["role"] = json!("wizard");
        assert!(validate(&task).is_err());

        let mut task = valid();
        task["owns"] = json!(["crates/polycore/src/api.rs", "crates/polycore/src/api.rs"]);
        assert!(validate(&task).is_err());
    }
}

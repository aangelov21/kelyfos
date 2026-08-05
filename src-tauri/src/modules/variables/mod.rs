//! Variable scoping and substitution.
//! Priority: terminal > workspace > global.

use std::collections::HashMap;

use crate::modules::storage::{self, Db};

pub fn resolve(db: &Db, workspace_id: Option<&str>, terminal_id: Option<&str>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for v in storage::variables_for(db, "global", None) {
        map.insert(v.key, v.value);
    }
    if let Some(ws) = workspace_id {
        for v in storage::variables_for(db, "workspace", Some(ws)) {
            map.insert(v.key, v.value);
        }
    }
    if let Some(t) = terminal_id {
        for v in storage::variables_for(db, "terminal", Some(t)) {
            map.insert(v.key, v.value);
        }
    }
    map
}

/// Replace `${VAR}` occurrences. Unknown variables are left untouched.
pub fn substitute(input: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        match rest[start..].find('}') {
            Some(end_rel) => {
                let key = &rest[start + 2..start + end_rel];
                match vars.get(key) {
                    Some(val) => out.push_str(val),
                    None => out.push_str(&rest[start..start + end_rel + 1]),
                }
                rest = &rest[start + end_rel + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[tauri::command]
pub fn variables_resolve(
    db: tauri::State<Db>,
    workspace_id: Option<String>,
    terminal_id: Option<String>,
) -> HashMap<String, String> {
    resolve(&db, workspace_id.as_deref(), terminal_id.as_deref())
}

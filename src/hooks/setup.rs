//! Install hail's hooks into the harness configs without disturbing anything
//! else: unknown keys, other tools' hooks, comments and layout survive. Any
//! earlier hail line (0.3's `[ -n "$TMUX_PANE" ] || exit 0; hail ...` form
//! included) is replaced in place of being duplicated. Pure: text in, text out.

use serde_json::{Map, Value, json};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, value};

use crate::error::{Error, Result};

/// The hooks hail wants, per harness: (event, command).
pub fn wanted(harness: &str) -> [(&'static str, String); 2] {
    [
        ("SessionStart", "hail brief --hook".to_string()),
        (
            "UserPromptSubmit",
            format!("hail deliver --format {harness}"),
        ),
    ]
}

fn is_hail_command(cmd: &str) -> bool {
    cmd.contains("hail deliver")
        || cmd.contains("hail brief")
        || cmd.contains("tmux-bridge deliver")
}

/// Merge into `~/.claude/settings.json`.
pub fn merge_claude(text: &str) -> Result<String> {
    let mut root: Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(text).map_err(|e| {
            Error::State(format!(
                "~/.claude/settings.json is not valid JSON ({e}); fix it, then rerun hail setup"
            ))
        })?
    };
    let Some(obj) = root.as_object_mut() else {
        return Err(Error::State(
            "~/.claude/settings.json is not a JSON object".into(),
        ));
    };
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        return Err(Error::State(
            "\"hooks\" in ~/.claude/settings.json is not an object".into(),
        ));
    };
    for (event, cmd) in wanted("claude") {
        merge_claude_event(hooks, event, &cmd);
    }
    let mut out = serde_json::to_string_pretty(&root).map_err(|e| Error::State(e.to_string()))?;
    out.push('\n');
    Ok(out)
}

fn merge_claude_event(hooks: &mut Map<String, Value>, event: &str, cmd: &str) {
    let groups = hooks.entry(event).or_insert_with(|| json!([]));
    if !groups.is_array() {
        *groups = json!([]);
    }
    let Some(groups) = groups.as_array_mut() else {
        return;
    };
    let command_of = |h: &Value| {
        h.get("command")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let ours: Vec<String> = groups
        .iter()
        .flat_map(|g| {
            g.get("hooks")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .map(|h| command_of(&h))
        .filter(|c| is_hail_command(c))
        .collect();
    if ours.len() == 1 && ours[0] == cmd {
        return;
    }
    for g in groups.iter_mut() {
        if let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) {
            hs.retain(|h| !is_hail_command(&command_of(h)));
        }
    }
    groups.retain(|g| {
        g.get("hooks")
            .and_then(Value::as_array)
            .is_none_or(|hs| !hs.is_empty())
    });
    groups.push(json!({ "hooks": [ { "type": "command", "command": cmd } ] }));
}

/// Merge into `~/.codex/config.toml`, keeping comments and layout.
pub fn merge_codex(text: &str) -> Result<String> {
    let mut doc: DocumentMut = text.parse().map_err(|e| {
        Error::State(format!(
            "~/.codex/config.toml does not parse ({e}); fix it, then rerun hail setup"
        ))
    })?;
    if doc.get("features").is_none() {
        doc["features"] = Item::Table(Table::new());
    }
    if doc["features"].get("hooks").and_then(Item::as_bool) != Some(true) {
        doc["features"]["hooks"] = value(true);
    }
    if doc.get("hooks").is_none() {
        let mut t = Table::new();
        t.set_implicit(true);
        doc["hooks"] = Item::Table(t);
    }
    let Some(hooks) = doc["hooks"].as_table_mut() else {
        return Err(Error::State(
            "[hooks] in ~/.codex/config.toml is not a table".into(),
        ));
    };
    for (event, cmd) in wanted("codex") {
        merge_codex_event(hooks, event, &cmd);
    }
    Ok(doc.to_string())
}

fn codex_commands(t: &Table) -> Vec<String> {
    t.get("hooks")
        .and_then(Item::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| {
                    v.as_inline_table()?
                        .get("command")?
                        .as_str()
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn merge_codex_event(hooks: &mut Table, event: &str, cmd: &str) {
    if hooks
        .get(event)
        .and_then(Item::as_array_of_tables)
        .is_none()
    {
        hooks.insert(event, Item::ArrayOfTables(ArrayOfTables::new()));
    }
    let Some(aot) = hooks.get_mut(event).and_then(Item::as_array_of_tables_mut) else {
        return;
    };
    let ours: Vec<String> = aot
        .iter()
        .flat_map(codex_commands)
        .filter(|c| is_hail_command(c))
        .collect();
    if ours.len() == 1 && ours[0] == cmd {
        return;
    }
    aot.retain(|t| !codex_commands(t).iter().any(|c| is_hail_command(c)));
    let mut entry = InlineTable::new();
    entry.insert("type", "command".into());
    entry.insert("command", cmd.into());
    let mut arr = Array::new();
    arr.push(entry);
    let mut t = Table::new();
    t.insert("hooks", value(arr));
    t.decor_mut().set_prefix(format!(
        "\n# hail: {}\n",
        if event == "SessionStart" {
            "the brief at session start"
        } else {
            "message bodies at prompt submit"
        }
    ));
    aot.push(t);
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_CLAUDE: &str = r#"{
  "model": "opus",
  "hooks": {
    "SessionStart": [
      { "hooks": [ { "type": "command", "command": "[ -n \"$TMUX_PANE\" ] || exit 0; hail brief" } ] },
      { "matcher": "x", "hooks": [ { "type": "command", "command": "other-tool" } ] }
    ],
    "UserPromptSubmit": [
      { "hooks": [ { "type": "command", "command": "[ -n \"$TMUX_PANE\" ] || exit 0; hail deliver --format claude" } ] }
    ]
  }
}"#;

    #[test]
    fn claude_replaces_old_lines_and_keeps_others() {
        let out = merge_claude(OLD_CLAUDE).unwrap();
        assert!(out.contains("\"hail brief --hook\""));
        assert!(out.contains("\"hail deliver --format claude\""));
        assert!(!out.contains("TMUX_PANE"));
        assert!(out.contains("other-tool"));
        assert!(out.find("\"model\"") < out.find("\"hooks\""));
        assert_eq!(merge_claude(&out).unwrap(), out, "idempotent");
    }

    #[test]
    fn claude_from_nothing() {
        let out = merge_claude("").unwrap();
        assert_eq!(out.matches("hail deliver").count(), 1);
    }

    const OLD_CODEX: &str = r#"model = "gpt-5"
# my own comment
[features]
hooks = true

[[hooks.Stop]]
hooks = [{ type = "command", command = "notify-me" }]

# hail: agent messaging — deliver bodies at prompt submit, brief at session start (silent outside tmux)
[[hooks.UserPromptSubmit]]
hooks = [{ type = "command", command = "[ -n \"$TMUX_PANE\" ] || exit 0; hail deliver --format codex" }]

[[hooks.SessionStart]]
hooks = [{ type = "command", command = "[ -n \"$TMUX_PANE\" ] || exit 0; hail brief" }]
"#;

    #[test]
    fn codex_replaces_old_lines_and_keeps_comments() {
        let out = merge_codex(OLD_CODEX).unwrap();
        assert!(out.contains("# my own comment"));
        assert!(out.contains("notify-me"));
        assert!(out.contains("command = \"hail deliver --format codex\""));
        assert!(out.contains("command = \"hail brief --hook\""));
        assert!(!out.contains("TMUX_PANE"));
        assert_eq!(merge_codex(&out).unwrap(), out, "idempotent");
    }

    #[test]
    fn codex_from_nothing_enables_hooks() {
        let out = merge_codex("").unwrap();
        let doc: DocumentMut = out.parse().unwrap();
        assert_eq!(doc["features"]["hooks"].as_bool(), Some(true));
        assert_eq!(out.matches("hail deliver").count(), 1);
    }
}

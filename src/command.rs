use std::path::Path;
use std::time::Instant;

use anyhow::Result;

use crate::app::App;
use crate::buffer::FileSource;
use crate::import;
use crate::search::{self, SearchPattern};

pub fn execute_command(app: &mut App, cmd: &str) -> Result<()> {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let command = parts[0];

    match command {
        "w" => {
            if parts.len() >= 2 {
                let path = parts[1];
                app.buffer.save_as(Path::new(path))?;
                app.buffer
                    .set_source(FileSource::Binary(std::path::PathBuf::from(path)));
                app.message = Some((format!("Saved as {}", path), Instant::now()));
            } else {
                app.buffer.save()?;
                app.message = Some(("Saved".to_string(), Instant::now()));
            }
        }
        "q" => {
            if app.buffer.is_dirty() {
                app.message = Some((
                    "No write since last change (add ! to override)".to_string(),
                    Instant::now(),
                ));
            } else {
                app.running = false;
            }
        }
        "q!" => {
            app.running = false;
        }
        "wq" => {
            app.buffer.save()?;
            app.running = false;
        }
        "import" => {
            if parts.len() >= 2 {
                let path = parts[1];
                app.buffer = crate::buffer::Buffer::from_hex_import(Path::new(path))?;
                app.cursor_offset = 0;
                app.scroll_offset = 0;
                app.undo_manager = crate::undo::UndoManager::new();
                app.message = Some((format!("Imported {}", path), Instant::now()));
            } else {
                app.message = Some(("Usage: :import <path>".to_string(), Instant::now()));
            }
        }
        "export" => {
            if parts.len() >= 2 {
                let path = parts[1];
                import::export_hex_file(app.buffer.get_range(0, app.buffer.len()), Path::new(path))?;
                app.message = Some((format!("Exported to {}", path), Instant::now()));
            } else {
                app.message = Some(("Usage: :export <path>".to_string(), Instant::now()));
            }
        }
        "goto" | "g" => {
            if parts.len() >= 2 {
                let offset = parse_offset(parts[1])?;
                app.cursor_offset = offset.min(app.buffer.len().saturating_sub(1));
            }
        }
        _ => {
            // 尝试解析为替换命令 :s/old/new 或 :%s/old/new/g
            if let Some((global, old, new)) = parse_substitute(trimmed) {
                match execute_substitute(app, global, old, new) {
                    Ok(msg) => {
                        app.message = Some((msg, Instant::now()));
                    }
                    Err(e) => {
                        app.message = Some((format!("Error: {}", e), Instant::now()));
                    }
                }
            } else {
                app.message = Some((format!("Unknown command: {}", command), Instant::now()));
            }
        }
    }

    Ok(())
}

fn parse_offset(s: &str) -> Result<usize> {
    if s.starts_with("0x") || s.starts_with("0X") {
        usize::from_str_radix(&s[2..], 16)
            .map_err(|e| anyhow::anyhow!("Invalid hex offset: {}", e))
    } else {
        s.parse::<usize>()
            .map_err(|e| anyhow::anyhow!("Invalid offset: {}", e))
    }
}

/// 解析替换命令，返回 (是否全局, old, new)
fn parse_substitute(cmd: &str) -> Option<(bool, &str, &str)> {
    let (global, rest) = if let Some(r) = cmd.strip_prefix("s/") {
        (false, r)
    } else if let Some(r) = cmd.strip_prefix("%s/") {
        (true, r)
    } else {
        return None;
    };

    let slash_idx = rest.find('/')?;
    let old = &rest[..slash_idx];
    let new_and_flag = &rest[slash_idx + 1..];

    let has_g_flag = new_and_flag.ends_with("/g");
    let new = if has_g_flag {
        &new_and_flag[..new_and_flag.len() - 2]
    } else {
        new_and_flag
    };

    // :s/old/new/g 和 :%s/old/new/g 都视为全局替换（兼容 vim 习惯）
    let global = global || has_g_flag;

    Some((global, old, new))
}

fn execute_substitute(app: &mut App, global: bool, old: &str, new: &str) -> Result<String> {
    let old_pat = search::parse_pattern(old)?;
    let new_bytes = search::parse_replacement(new)?;

    if global {
        search::replace_all(app, &old_pat, &new_bytes)?;
        Ok("Replaced all".to_string())
    } else {
        // 当前匹配替换：如果当前没有搜索状态或模式不同，先搜索
        let need_search = app.search_state.pattern.as_ref().map_or(true, |p| {
            !patterns_equal(p, &old_pat)
        });

        if need_search {
            app.search_state.search(&app.buffer, old_pat.clone());
            // 选中从当前光标开始的第一个匹配
            if let Some(offset) = app.search_state.next_match(app.cursor_offset) {
                app.cursor_offset = offset;
            }
        }

        search::replace_current(app, &new_bytes)?;
        Ok("Replaced".to_string())
    }
}

fn patterns_equal(a: &SearchPattern, b: &SearchPattern) -> bool {
    match (a, b) {
        (SearchPattern::Hex(a), SearchPattern::Hex(b)) => a == b,
        (SearchPattern::Ascii(a), SearchPattern::Ascii(b)) => a == b,
        _ => false,
    }
}

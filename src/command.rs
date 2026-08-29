use std::path::Path;
use std::time::Instant;

use anyhow::{Result, bail};

use crate::app::{App, Mode};
use crate::buffer::FileSource;
use crate::checksum::{self, ChecksumInfo};
use crate::frame::{ViewMode, FrameConfig, build_frame_index};
use crate::import;
use crate::search::{self, SearchPattern};

pub fn execute_command(app: &mut App, cmd: &str) -> Result<()> {
    // Visual 模式按 `:` 进入时暂存的选区范围：任何命令执行后即消费（仅校验和命令使用）
    let pending_range = app.pending_range.take();

    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let command = parts[0];

    match command {
        "w" => {
            if let Some(msg) = check_frame_length(app) {
                app.message = Some((msg, Instant::now()));
                return Ok(());
            }
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
        "w!" => {
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
        "wq" => {
            if let Some(msg) = check_frame_length(app) {
                app.message = Some((msg, Instant::now()));
                return Ok(());
            }
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
                // 跳转前记录当前光标位置到 jumplist（偏移有变化时才入栈）
                let target = offset.min(app.buffer.len().saturating_sub(1));
                if target != app.cursor_offset {
                    app.push_jump();
                }
                app.cursor_offset = target;
            }
        }
        "help" | "h" => {
            app.help_topic = parts.get(1).map(|s| s.to_string());
            app.help_scroll = 0;
            app.mode = Mode::Help;
        }
        "sum" | "checksum" => {
            let range = checksum_range(app, pending_range);
            app.sum_info = Some(make_checksum_info(app, range));
            app.sum_open = true;
        }
        "crc16" => {
            match parse_crc_args(16, &parts[1..]) {
                Ok((params, label)) => {
                    let data = checksum_data(app, checksum_range(app, pending_range));
                    let value = checksum::crc(data, &params) as u16;
                    app.message = Some((
                        format!("CRC16 ({}): {:04X}", label, value),
                        Instant::now(),
                    ));
                }
                Err(msg) => {
                    app.message = Some((msg, Instant::now()));
                }
            }
        }
        "crc32" => {
            match parse_crc_args(32, &parts[1..]) {
                Ok((params, label)) => {
                    let data = checksum_data(app, checksum_range(app, pending_range));
                    let value = checksum::crc(data, &params) as u32;
                    app.message = Some((
                        format!("CRC32 ({}): {:08X}", label, value),
                        Instant::now(),
                    ));
                }
                Err(msg) => {
                    app.message = Some((msg, Instant::now()));
                }
            }
        }
        "md5" => {
            let data = checksum_data(app, checksum_range(app, pending_range));
            app.message = Some((format!("MD5: {}", checksum::md5(data)), Instant::now()));
        }
        "sha256" => {
            let data = checksum_data(app, checksum_range(app, pending_range));
            app.message = Some((format!("SHA256: {}", checksum::sha256(data)), Instant::now()));
        }
        "sum8" => {
            let data = checksum_data(app, checksum_range(app, pending_range));
            app.message = Some((format!("SUM8: {:02X}", checksum::sum8(data)), Instant::now()));
        }
        "sum16" => {
            let data = checksum_data(app, checksum_range(app, pending_range));
            let endian = if app.type_endian_le { "LE" } else { "BE" };
            app.message = Some((
                format!("SUM16 ({}): {:04X}", endian, checksum::sum16(data, app.type_endian_le)),
                Instant::now(),
            ));
        }
        "sum32" => {
            let data = checksum_data(app, checksum_range(app, pending_range));
            let endian = if app.type_endian_le { "LE" } else { "BE" };
            app.message = Some((
                format!("SUM32 ({}): {:08X}", endian, checksum::sum32(data, app.type_endian_le)),
                Instant::now(),
            ));
        }
        "frame" => {
            if parts.len() >= 2 {
                let arg = parts[1];
                if arg == "off" {
                    app.view_mode = ViewMode::Raw;
                    app.frame_index = None;
                    app.frame_original_len = None;
                    app.h_scroll_offset = 0;
                    app.message = Some(("Frame mode off".to_string(), Instant::now()));
                } else if let Some(rest) = arg.strip_prefix("len=") {
                    match parse_offset(rest) {
                        Ok(length) => {
                            if length > 0 {
                                let config = FrameConfig::FixedLength { length };
                                let index = build_frame_index(app.buffer.data(), &config);
                                app.frame_index = Some(index);
                                app.frame_original_len = Some(app.buffer.len());
                                app.view_mode = ViewMode::Frame;
                                app.message = Some((format!("Frame mode: fixed length {}", length), Instant::now()));
                            } else {
                                app.message = Some(("Frame length must be > 0".to_string(), Instant::now()));
                            }
                        }
                        Err(e) => {
                            app.message = Some((format!("Invalid frame length: {}", e), Instant::now()));
                        }
                    }
                } else if let Some(rest) = arg.strip_prefix("sync=") {
                    match parse_hex_bytes(rest) {
                        Ok(pattern) if !pattern.is_empty() => {
                            let config = FrameConfig::SyncWord { pattern };
                            let index = build_frame_index(app.buffer.data(), &config);
                            app.frame_index = Some(index);
                            app.view_mode = ViewMode::Frame;
                            app.message = Some(("Frame mode: sync word".to_string(), Instant::now()));
                        }
                        Ok(_) => {
                            app.message = Some(("Sync word pattern must not be empty".to_string(), Instant::now()));
                        }
                        Err(e) => {
                            app.message = Some((format!("Invalid sync word: {}", e), Instant::now()));
                        }
                    }
                } else {
                    app.message = Some((format!("Unknown frame argument: {}", arg), Instant::now()));
                }
            } else {
                app.message = Some(("Usage: :frame len=N | :frame sync=HEX | :frame off".to_string(), Instant::now()));
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

/// 校验和计算范围：Visual 选区（含从 Visual 进入 Command 时暂存的 pending_range）优先，
/// 否则全文。返回含两端的 (start, end)；空缓冲区返回 (0, 0)。
fn checksum_range(app: &App, pending_range: Option<(usize, usize)>) -> (usize, usize) {
    if app.buffer.is_empty() {
        return (0, 0);
    }
    pending_range
        .or_else(|| app.selection_range())
        .unwrap_or((0, app.buffer.len() - 1))
}

/// 按范围取字节切片（含两端），越界由 get_range 钳制，空缓冲区返回空切片
fn checksum_data(app: &App, range: (usize, usize)) -> &[u8] {
    let (start, end) = range;
    if app.buffer.is_empty() || end < start {
        return &[];
    }
    app.buffer.get_range(start, end - start + 1)
}

/// 构造校验和浮层快照：CRC16 用 CCITT-FALSE、CRC32 用 IEEE；
/// SUM16/SUM32 按当前全局端序（type_endian_le）取字，端序随快照记录以便浮层标注。
fn make_checksum_info(app: &App, range: (usize, usize)) -> ChecksumInfo {
    let data = checksum_data(app, range);
    let le = app.type_endian_le;
    ChecksumInfo {
        range,
        len: data.len(),
        crc16: format!("{:04X}", checksum::crc16(data)),
        crc32: format!("{:08X}", checksum::crc32(data)),
        md5: checksum::md5(data),
        sha256: checksum::sha256(data),
        sum8: format!("{:02X}", checksum::sum8(data)),
        sum16: format!("{:04X}", checksum::sum16(data, le)),
        sum32: format!("{:08X}", checksum::sum32(data, le)),
        sum_le: le,
    }
}

/// 解析 `:crc16` / `:crc32` 参数：无参数用默认预设；单词为预设名；
/// 键值对（可叠加在预设基底上，也可纯自定义）覆盖参数。
/// 成功时返回 (参数, 消息行标签)；解析错误返回可直接展示的错误消息。
fn parse_crc_args(width: u32, args: &[&str]) -> Result<(checksum::CrcParams, String), String> {
    let crc_name = if width == 16 { "CRC16" } else { "CRC32" };
    let preset_names = if width == 16 {
        checksum::crc16_preset_names()
    } else {
        checksum::crc32_preset_names()
    };
    let usage = format!(
        "Usage: :crc{} [preset] [poly= init= refin= refout= xorout=] (presets: {})",
        width, preset_names
    );

    let mut params;
    let mut preset_name: Option<String> = None;
    let mut kv_start = 0;

    if let Some(first) = args.first() {
        if !first.contains('=') {
            let base = if width == 16 {
                checksum::crc16_preset(first)
            } else {
                checksum::crc32_preset(first)
            };
            match base {
                Some(p) => {
                    params = checksum::CrcParams {
                        width,
                        poly: p.poly,
                        init: p.init,
                        refin: p.refin,
                        refout: p.refout,
                        xorout: p.xorout,
                    };
                    preset_name = Some(first.to_string());
                }
                None => {
                    return Err(format!(
                        "Unknown {} preset: {} (available: {})",
                        crc_name, first, preset_names
                    ));
                }
            }
            kv_start = 1;
        } else {
            // 纯自定义：未提供的参数用合理默认（poly 必须显式给出）
            params = checksum::CrcParams {
                width,
                poly: 0,
                init: 0,
                refin: false,
                refout: false,
                xorout: 0,
            };
        }
    } else {
        let default_name = if width == 16 { "ccitt-false" } else { "ieee" };
        let p = if width == 16 {
            checksum::crc16_preset(default_name)
        } else {
            checksum::crc32_preset(default_name)
        };
        let p = p.expect("内置默认预设必须存在");
        return Ok((
            checksum::CrcParams {
                width,
                poly: p.poly,
                init: p.init,
                refin: p.refin,
                refout: p.refout,
                xorout: p.xorout,
            },
            default_name.to_ascii_uppercase(),
        ));
    }

    let mask = (1u64 << width) - 1;
    let mut provided = [false; 5]; // poly / init / refin / refout / xorout
    for arg in &args[kv_start..] {
        let (key, value) = match arg.split_once('=') {
            Some(pair) => pair,
            None => return Err(format!("Invalid argument: {}. {}", arg, usage)),
        };
        match key {
            "poly" => {
                params.poly = parse_crc_number(value).ok_or_else(|| {
                    format!("Invalid poly value: {}. {}", value, usage)
                })?;
                provided[0] = true;
            }
            "init" => {
                params.init = parse_crc_number(value).ok_or_else(|| {
                    format!("Invalid init value: {}. {}", value, usage)
                })?;
                provided[1] = true;
            }
            "refin" => {
                params.refin = parse_bool_arg(value).ok_or_else(|| {
                    format!("Invalid refin value: {}. {}", value, usage)
                })?;
                provided[2] = true;
            }
            "refout" => {
                params.refout = parse_bool_arg(value).ok_or_else(|| {
                    format!("Invalid refout value: {}. {}", value, usage)
                })?;
                provided[3] = true;
            }
            "xorout" => {
                params.xorout = parse_crc_number(value).ok_or_else(|| {
                    format!("Invalid xorout value: {}. {}", value, usage)
                })?;
                provided[4] = true;
            }
            _ => return Err(format!("Unknown parameter: {}. {}", key, usage)),
        }
    }

    // 校验：数值参数不超宽度掩码；poly 非零（纯自定义时必须显式提供）
    for (name, value) in [
        ("poly", params.poly),
        ("init", params.init),
        ("xorout", params.xorout),
    ] {
        if value > mask {
            return Err(format!(
                "{} value 0x{:X} exceeds {}-bit width. {}",
                name, value, width, usage
            ));
        }
    }
    if params.poly == 0 {
        return Err(format!("poly must be non-zero. {}", usage));
    }

    let label = crc_label(crc_name, preset_name, &provided, &params);
    Ok((params, label))
}

/// 消息行标签：纯预设时显示预设名；含覆盖/纯自定义时列出提供的参数。
/// 参数较多时省略反射类标签，保持单行可读（数值类优先）。
fn crc_label(
    crc_name: &str,
    preset_name: Option<String>,
    provided: &[bool; 5],
    p: &checksum::CrcParams,
) -> String {
    if provided.iter().all(|&x| !x) {
        return preset_name
            .map(|n| n.to_ascii_uppercase())
            .unwrap_or_else(|| crc_name.to_string());
    }
    let mut segs: Vec<String> = Vec::new();
    if provided[0] {
        segs.push(format!("poly=0x{:X}", p.poly));
    }
    if provided[1] {
        segs.push(format!("init=0x{:X}", p.init));
    }
    // 参数较多时省略反射类标签（数值类优先），保持单行可读：总段数上限 4
    let numeric_count = segs.len() + usize::from(provided[4]);
    let reflect_count = usize::from(provided[2]) + usize::from(provided[3]);
    let keep_reflect = numeric_count + reflect_count <= 4;
    if keep_reflect && provided[2] {
        segs.push(format!("refin={}", p.refin));
    }
    if keep_reflect && provided[3] {
        segs.push(format!("refout={}", p.refout));
    }
    if provided[4] {
        segs.push(format!("xorout=0x{:X}", p.xorout));
    }
    // 预设基底 + 覆盖：标签前缀保留预设名，如 "IEEE xorout=0x0"
    match preset_name {
        Some(name) => format!("{} {}", name.to_ascii_uppercase(), segs.join(" ")),
        None => segs.join(" "),
    }
}

/// 数值解析：支持 0x 前缀十六进制与十进制（含 0X 前缀，与 :goto 一致）
fn parse_crc_number(s: &str) -> Option<u64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

/// 布尔解析：true / false（不区分大小写）
fn parse_bool_arg(s: &str) -> Option<bool> {
    if s.eq_ignore_ascii_case("true") {
        Some(true)
    } else if s.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// 检查固定长度帧模式下 buffer 长度是否发生变化
/// 如果发生变化，返回警告消息；否则返回 None
fn check_frame_length(app: &App) -> Option<String> {
    let is_fixed_length = app
        .frame_index
        .as_ref()
        .map(|fi| matches!(fi.config, FrameConfig::FixedLength { .. }))
        .unwrap_or(false);
    if !is_fixed_length {
        return None;
    }
    if let Some(original_len) = app.frame_original_len {
        let current_len = app.buffer.len();
        if current_len != original_len {
            return Some(format!(
                "Length changed (was {}, now {}). Use :w! to force save",
                original_len, current_len
            ));
        }
    }
    None
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
            let data = app.buffer.data().to_vec();
            app.search_state.start_search(data, old_pat.clone());
            // 对于替换操作，需要等待搜索完成后才能替换
            // 等待异步搜索完成（最多等待 5 秒）
            let wait_start = std::time::Instant::now();
            while app.search_state.is_searching() {
                if wait_start.elapsed() > std::time::Duration::from_secs(5) {
                    app.search_state.cancel();
                    anyhow::bail!("Search timed out");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            app.search_state.poll_result();
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

fn parse_hex_bytes(hex_str: &str) -> Result<Vec<u8>> {
    let cleaned: String = hex_str.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        bail!("Empty hex string");
    }
    if cleaned.len() % 2 != 0 {
        bail!("Hex string must have even number of digits");
    }
    let mut bytes = Vec::with_capacity(cleaned.len() / 2);
    for i in (0..cleaned.len()).step_by(2) {
        let byte = u8::from_str_radix(&cleaned[i..i + 2], 16)
            .map_err(|e| anyhow::anyhow!("Invalid hex byte: {}", e))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;

    /// 构造带数据的 App（直接构造 Buffer 避免留下编辑/undo 记录）
    fn app_with_data(data: &[u8]) -> App {
        let mut app = App::new();
        app.buffer = Buffer::with_data(data);
        app
    }

    /// `:crc32` 无选区时计算全文（默认 ieee），消息行显示 8 位大写 hex（"abc" 已知向量）
    #[test]
    fn crc32_command_shows_full_file_checksum() {
        let mut app = app_with_data(b"abc");
        execute_command(&mut app, "crc32").unwrap();
        let (msg, _) = app.message.as_ref().expect("应有消息行");
        assert_eq!(msg, "CRC32 (IEEE): 352441C2");
    }

    /// `:crc32 [preset]` 可选 c / stm32（"123456789" 已知向量），预设名不区分大小写；
    /// 未知预设报错并列出可用预设名（不消费选区状态由顶部统一清理覆盖）
    #[test]
    fn crc32_command_presets() {
        let mut app = app_with_data(b"123456789");
        execute_command(&mut app, "crc32 c").unwrap();
        assert_eq!(app.message.as_ref().unwrap().0, "CRC32 (C): E3069283");

        execute_command(&mut app, "crc32 IEEE").unwrap();
        assert_eq!(app.message.as_ref().unwrap().0, "CRC32 (IEEE): CBF43926");

        execute_command(&mut app, "crc32 nope").unwrap();
        let (msg, _) = app.message.as_ref().unwrap();
        assert!(msg.contains("Unknown CRC32 preset: nope"), "实际: {}", msg);
        assert!(msg.contains("ieee, c, stm32"), "实际: {}", msg);
    }

    /// `:crc16` 默认 ccitt-false，消息行 4 位大写 hex（"123456789" 已知向量）
    #[test]
    fn crc16_command_defaults_and_presets() {
        let mut app = app_with_data(b"123456789");
        execute_command(&mut app, "crc16").unwrap();
        assert_eq!(app.message.as_ref().unwrap().0, "CRC16 (CCITT-FALSE): 29B1");

        execute_command(&mut app, "crc16 xmodem").unwrap();
        assert_eq!(app.message.as_ref().unwrap().0, "CRC16 (XMODEM): 31C3");

        execute_command(&mut app, "crc16 MODBUS").unwrap();
        assert_eq!(app.message.as_ref().unwrap().0, "CRC16 (MODBUS): 4B37");

        execute_command(&mut app, "crc16 arc").unwrap();
        assert_eq!(app.message.as_ref().unwrap().0, "CRC16 (ARC): BB3D");

        execute_command(&mut app, "crc16 nope").unwrap();
        let (msg, _) = app.message.as_ref().unwrap();
        assert!(msg.contains("Unknown CRC16 preset: nope"), "实际: {}", msg);
        assert!(
            msg.contains("ccitt-false, xmodem, modbus, arc"),
            "实际: {}",
            msg
        );
    }

    /// 自定义参数：纯键值对、预设基底覆盖、十进制数值（"123456789" 已知向量）
    #[test]
    fn crc_commands_custom_params() {
        let mut app = app_with_data(b"123456789");

        // 纯自定义 == CCITT-FALSE
        execute_command(&mut app, "crc16 poly=0x1021 init=0xFFFF").unwrap();
        assert_eq!(
            app.message.as_ref().unwrap().0,
            "CRC16 (poly=0x1021 init=0xFFFF): 29B1"
        );

        // 预设基底覆盖：ieee xorout=0 → CBF43926 ^ FFFFFFFF
        execute_command(&mut app, "crc32 ieee xorout=0").unwrap();
        assert_eq!(
            app.message.as_ref().unwrap().0,
            "CRC32 (IEEE xorout=0x0): 340BC6D9"
        );

        // 纯自定义 == MODBUS（反射开关也在标签中）
        execute_command(&mut app, "crc16 poly=0x8005 init=0xFFFF refin=true refout=true").unwrap();
        assert_eq!(
            app.message.as_ref().unwrap().0,
            "CRC16 (poly=0x8005 init=0xFFFF refin=true refout=true): 4B37"
        );

        // 十进制数值等价于 0x1021 / 0xFFFF
        execute_command(&mut app, "crc16 poly=4129 init=65535").unwrap();
        assert_eq!(
            app.message.as_ref().unwrap().0,
            "CRC16 (poly=0x1021 init=0xFFFF): 29B1"
        );
    }

    /// 非法参数：未知键 / 非法数字 / 超宽值 / 零多项式 / 非法布尔，均报错不 panic 并提示用法；
    /// 预设基底 + 非法键不改变后续命令行为（无状态）
    #[test]
    fn crc_commands_invalid_custom_params() {
        let mut app = app_with_data(b"123456789");

        execute_command(&mut app, "crc16 foo=1").unwrap();
        let msg = &app.message.as_ref().unwrap().0;
        assert!(msg.contains("Unknown parameter: foo"), "实际: {}", msg);
        assert!(msg.contains("Usage: :crc16"), "实际: {}", msg);

        execute_command(&mut app, "crc16 poly=zz").unwrap();
        let msg = &app.message.as_ref().unwrap().0;
        assert!(msg.contains("Invalid poly value: zz"), "实际: {}", msg);
        assert!(msg.contains("Usage: :crc16"), "实际: {}", msg);

        execute_command(&mut app, "crc16 poly=0x10000").unwrap();
        let msg = &app.message.as_ref().unwrap().0;
        assert!(msg.contains("exceeds 16-bit width"), "实际: {}", msg);

        execute_command(&mut app, "crc32 init=0x100000000").unwrap();
        let msg = &app.message.as_ref().unwrap().0;
        assert!(msg.contains("exceeds 32-bit width"), "实际: {}", msg);

        execute_command(&mut app, "crc16 init=0xFFFF").unwrap();
        let msg = &app.message.as_ref().unwrap().0;
        assert!(msg.contains("poly must be non-zero"), "实际: {}", msg);

        execute_command(&mut app, "crc16 refin=yes").unwrap();
        let msg = &app.message.as_ref().unwrap().0;
        assert!(msg.contains("Invalid refin value: yes"), "实际: {}", msg);

        execute_command(&mut app, "crc32 0x1234").unwrap();
        let msg = &app.message.as_ref().unwrap().0;
        assert!(msg.contains("Unknown CRC32 preset: 0x1234"), "实际: {}", msg);

        // 报错后命令仍可正常使用（无残留状态）
        execute_command(&mut app, "crc16").unwrap();
        assert_eq!(
            app.message.as_ref().unwrap().0,
            "CRC16 (CCITT-FALSE): 29B1"
        );
    }

    /// 参数较多时标签省略反射类，保持单行可读（数值类优先）
    #[test]
    fn crc_command_label_truncates_reflect_flags() {
        let mut app = app_with_data(b"123456789");
        execute_command(
            &mut app,
            "crc16 poly=0x8005 init=0xFFFF xorout=0x0 refin=true refout=true",
        )
        .unwrap();
        assert_eq!(
            app.message.as_ref().unwrap().0,
            "CRC16 (poly=0x8005 init=0xFFFF xorout=0x0): 4B37"
        );
    }

    /// `:md5` / `:sha256` 全文已知向量（"abc"）
    #[test]
    fn md5_and_sha256_commands_show_full_file_checksum() {
        let mut app = app_with_data(b"abc");
        execute_command(&mut app, "md5").unwrap();
        let (msg, _) = app.message.as_ref().unwrap();
        assert_eq!(msg, "MD5: 900150983cd24fb0d6963f7d28e17f72");

        execute_command(&mut app, "sha256").unwrap();
        let (msg, _) = app.message.as_ref().unwrap();
        assert_eq!(
            msg,
            "SHA256: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// Visual 选区激活时 `:md5` 只计算选区字节（选区 1..=3 即 "abc"）
    #[test]
    fn md5_command_uses_visual_selection() {
        let mut app = app_with_data(b"xabc");
        app.visual_anchor = Some(1);
        app.cursor_offset = 3;
        execute_command(&mut app, "md5").unwrap();
        let (msg, _) = app.message.as_ref().unwrap();
        assert_eq!(msg, "MD5: 900150983cd24fb0d6963f7d28e17f72");
    }

    /// pending_range（Visual → Command 暂存选区）优先于当前光标，执行后清空；
    /// 即使之后光标/anchor 已失效，仍按暂存范围计算（"abc" 已知向量）
    #[test]
    fn pending_range_takes_priority_and_is_cleared() {
        let mut app = app_with_data(b"xabc");
        app.pending_range = Some((1, 3));
        execute_command(&mut app, "sha256").unwrap();
        let (msg, _) = app.message.as_ref().unwrap();
        assert_eq!(
            msg,
            "SHA256: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(app.pending_range, None, "命令执行后应清空 pending_range");
    }

    /// 非校验和命令也会清空 pending_range，避免过期范围污染后续命令
    #[test]
    fn pending_range_cleared_by_any_command() {
        let mut app = app_with_data(&[0u8; 64]);
        app.pending_range = Some((1, 3));
        execute_command(&mut app, "goto 10").unwrap();
        assert_eq!(app.pending_range, None);
    }

    /// `:sum` 打开浮层并缓存当前范围的三种校验和；`:checksum` 为别名；
    /// q/Esc 关闭由 input 层测试覆盖，此处验证标志与快照内容（"abc" 已知向量）
    #[test]
    fn sum_command_opens_panel_with_results() {
        let mut app = app_with_data(b"abc");
        execute_command(&mut app, "sum").unwrap();
        assert!(app.sum_open, ":sum 应打开校验和浮层");
        let info = app.sum_info.as_ref().expect("应有校验和快照");
        assert_eq!(info.range, (0, 2));
        assert_eq!(info.len, 3);
        // 浮层固定预设：CRC16 = CCITT-FALSE，CRC32 = IEEE
        assert_eq!(info.crc16, "514A");
        assert_eq!(info.crc32, "352441C2");
        assert_eq!(info.md5, "900150983cd24fb0d6963f7d28e17f72");
        // 累加和："abc" = [0x61, 0x62, 0x63]，默认 LE
        assert_eq!(info.sum8, "26");
        assert_eq!(info.sum16, "62C4");
        assert_eq!(info.sum32, "00636261");
        assert!(info.sum_le, "默认端序应为 LE");

        // 全局端序切到 BE 后重新打开 :sum，SUM16/SUM32 按 BE 取字并记录端序
        app.sum_open = false;
        app.type_endian_le = false;
        execute_command(&mut app, "sum").unwrap();
        let info = app.sum_info.as_ref().unwrap();
        assert_eq!(info.sum16, "C462"); // 0x6162 + 0x6300
        assert_eq!(info.sum32, "61626300");
        assert!(!info.sum_le);
        app.type_endian_le = true;

        app.sum_open = false;
        execute_command(&mut app, "checksum").unwrap();
        assert!(app.sum_open, ":checksum 别名应同样打开浮层");
    }

    /// Visual 选区下 `:sum` 只对选区计算（选区 1..=3 即 "abc"）
    #[test]
    fn sum_command_uses_visual_selection() {
        let mut app = app_with_data(b"xabc");
        app.visual_anchor = Some(3);
        app.cursor_offset = 1; // anchor > cursor，验证范围自动排序
        execute_command(&mut app, "sum").unwrap();
        let info = app.sum_info.as_ref().unwrap();
        assert_eq!(info.range, (1, 3));
        assert_eq!(info.crc32, "352441C2");
    }

    /// 空缓冲区：校验和命令不 panic，按空输入计算（空向量已知值）
    #[test]
    fn checksum_commands_on_empty_buffer() {
        let mut app = App::new();
        execute_command(&mut app, "crc32").unwrap();
        let (msg, _) = app.message.as_ref().unwrap();
        assert_eq!(msg, "CRC32 (IEEE): 00000000");

        execute_command(&mut app, "crc16").unwrap();
        let (msg, _) = app.message.as_ref().unwrap();
        assert_eq!(msg, "CRC16 (CCITT-FALSE): FFFF");

        execute_command(&mut app, "sum").unwrap();
        assert!(app.sum_open);
        let info = app.sum_info.as_ref().unwrap();
        assert_eq!(info.len, 0);
        assert_eq!(info.crc16, "FFFF");
        assert_eq!(info.md5, "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(info.sum8, "00");
        assert_eq!(info.sum16, "0000");
        assert_eq!(info.sum32, "00000000");
    }

    /// `:sum8` / `:sum16` / `:sum32` 消息行格式；SUM16/32 端序随全局设置（type_endian_le）。
    /// 数据 [0x01, 0x02, 0x03]：SUM8=06；LE: SUM16=0204 / SUM32=00030201；BE: 0402 / 01020300
    #[test]
    fn sum8_sum16_sum32_commands() {
        let mut app = app_with_data(&[0x01, 0x02, 0x03]);
        execute_command(&mut app, "sum8").unwrap();
        assert_eq!(app.message.as_ref().unwrap().0, "SUM8: 06");

        app.type_endian_le = true;
        execute_command(&mut app, "sum16").unwrap();
        assert_eq!(app.message.as_ref().unwrap().0, "SUM16 (LE): 0204");
        execute_command(&mut app, "sum32").unwrap();
        assert_eq!(app.message.as_ref().unwrap().0, "SUM32 (LE): 00030201");

        app.type_endian_le = false;
        execute_command(&mut app, "sum16").unwrap();
        assert_eq!(app.message.as_ref().unwrap().0, "SUM16 (BE): 0402");
        execute_command(&mut app, "sum32").unwrap();
        assert_eq!(app.message.as_ref().unwrap().0, "SUM32 (BE): 01020300");
    }

    /// Visual 选区下 `:sum16` 只对选区计算（选区 1..=3 即 [0x01, 0x02, 0x03]）
    #[test]
    fn sum16_command_uses_visual_selection() {
        let mut app = app_with_data(&[0xAA, 0x01, 0x02, 0x03]);
        app.visual_anchor = Some(1);
        app.cursor_offset = 3;
        execute_command(&mut app, "sum16").unwrap();
        assert_eq!(app.message.as_ref().unwrap().0, "SUM16 (LE): 0204");
    }
}

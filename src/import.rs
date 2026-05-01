use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};

/// 将单个 hex 字符转为 4-bit 值
#[inline]
fn hex_char_to_u8(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// 解析 hex 文本文件，返回二进制数据
///
/// 输入文件每行包含空格分隔的十六进制字符串。
/// 忽略空行和 `#` 开头的注释行。
/// 每个 hex 块必须为偶数长度字符，支持大小写混合。
pub fn parse_hex_file(path: &Path) -> Result<Vec<u8>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read hex file: {}", path.display()))?;

    // 预分配：文本中最多一半字符是有效 hex（另一半是空白/分隔符）
    let mut data = Vec::with_capacity(content.len() / 2);

    for (line_num, line) in content.lines().enumerate() {
        let line_no = line_num + 1;
        let trimmed = line.trim();

        // 忽略空行
        if trimmed.is_empty() {
            continue;
        }

        // 忽略注释行
        if trimmed.starts_with('#') {
            continue;
        }

        // 按空白字符分割为多个 hex 块
        for chunk in trimmed.split_whitespace() {
            // 检查长度是否为偶数
            if chunk.len() % 2 != 0 {
                anyhow::bail!(
                    "Hex chunk has odd length at line {}: '{}'",
                    line_no,
                    chunk
                );
            }

            // 逐字节解析（直接操作 ASCII 字节，避免 from_str_radix 和 chars 迭代器开销）
            let chunk_bytes = chunk.as_bytes();
            for i in (0..chunk_bytes.len()).step_by(2) {
                let hi = hex_char_to_u8(chunk_bytes[i]).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Invalid hex character '{}' at line {} in chunk '{}'",
                        chunk_bytes[i] as char,
                        line_no,
                        chunk
                    )
                })?;
                let lo = hex_char_to_u8(chunk_bytes[i + 1]).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Invalid hex character '{}' at line {} in chunk '{}'",
                        chunk_bytes[i + 1] as char,
                        line_no,
                        chunk
                    )
                })?;
                data.push((hi << 4) | lo);
            }
        }
    }

    Ok(data)
}

/// 将二进制数据导出为 hex 文本格式
///
/// 每行输出 16 字节（32 个 hex 字符），空格分隔每 8 字节（16 个 hex 字符）。
/// 大写 hex 字符，最后一行可能不足 16 字节。
pub fn export_hex_file(data: &[u8], path: &Path) -> Result<()> {
    let file = fs::File::create(path)
        .with_context(|| format!("Failed to create hex export file: {}", path.display()))?;
    let mut writer = BufWriter::new(file);

    for (i, chunk) in data.chunks(16).enumerate() {
        if i > 0 {
            writeln!(writer)?;
        }

        // 每 16 字节再按每 8 字节分组
        for (j, byte_chunk) in chunk.chunks(8).enumerate() {
            if j > 0 {
                write!(writer, " ")?;
            }
            for byte in byte_chunk {
                write!(writer, "{:02X}", byte)?;
            }
        }
    }
    writeln!(writer)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_file(content: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir();
        let name = format!(
            "hrush_test_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let path = dir.join(&name);
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    fn cleanup(path: &std::path::Path) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_parse_single_line() {
        let path = create_temp_file("0A0C1537AABBCCDD CCEEAABB01020304");
        let data = parse_hex_file(&path).unwrap();
        cleanup(&path);
        assert_eq!(
            data,
            vec![0x0A, 0x0C, 0x15, 0x37, 0xAA, 0xBB, 0xCC, 0xDD, 0xCC, 0xEE, 0xAA, 0xBB, 0x01, 0x02, 0x03, 0x04]
        );
    }

    #[test]
    fn test_parse_multi_line() {
        let content = "0A0C1537AABBCCDD\nCCEEAABB01020304";
        let path = create_temp_file(content);
        let data = parse_hex_file(&path).unwrap();
        cleanup(&path);
        assert_eq!(
            data,
            vec![0x0A, 0x0C, 0x15, 0x37, 0xAA, 0xBB, 0xCC, 0xDD, 0xCC, 0xEE, 0xAA, 0xBB, 0x01, 0x02, 0x03, 0x04]
        );
    }

    #[test]
    fn test_parse_ignores_empty_lines() {
        let content = "\n\n0A0C\n\n1537\n\n";
        let path = create_temp_file(content);
        let data = parse_hex_file(&path).unwrap();
        cleanup(&path);
        assert_eq!(data, vec![0x0A, 0x0C, 0x15, 0x37]);
    }

    #[test]
    fn test_parse_ignores_comments() {
        let content = "# This is a comment\n0A0C\n# Another comment\n1537";
        let path = create_temp_file(content);
        let data = parse_hex_file(&path).unwrap();
        cleanup(&path);
        assert_eq!(data, vec![0x0A, 0x0C, 0x15, 0x37]);
    }

    #[test]
    fn test_parse_mixed_case() {
        let path = create_temp_file("aAbBcCdD");
        let data = parse_hex_file(&path).unwrap();
        cleanup(&path);
        assert_eq!(data, vec![0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn test_parse_odd_length_error() {
        let path = create_temp_file("0A0C1");
        let result = parse_hex_file(&path);
        cleanup(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("odd length"), "Error should mention odd length: {}", err);
        assert!(err.contains("0A0C1"), "Error should contain the chunk: {}", err);
        assert!(err.contains("line 1"), "Error should contain line number: {}", err);
    }

    #[test]
    fn test_parse_invalid_character_error() {
        let path = create_temp_file("0A0G");
        let result = parse_hex_file(&path);
        cleanup(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid hex character 'G'"), "Error should mention invalid char: {}", err);
    }

    #[test]
    fn test_export_basic() {
        let data: Vec<u8> = (0..32).collect();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("hrush_export_test_{}", std::process::id()));

        export_hex_file(&data, &path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "0001020304050607 08090A0B0C0D0E0F");
        assert_eq!(lines[1], "1011121314151617 18191A1B1C1D1E1F");
        cleanup(&path);
    }

    #[test]
    fn test_export_short_last_line() {
        let data = vec![0x00, 0x01, 0x02, 0x03];
        let dir = std::env::temp_dir();
        let path = dir.join(format!("hrush_export_short_test_{}", std::process::id()));

        export_hex_file(&data, &path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "00010203");
        cleanup(&path);
    }

    #[test]
    fn test_export_exactly_8_bytes() {
        let data: Vec<u8> = (0..8).collect();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("hrush_export_8_test_{}", std::process::id()));

        export_hex_file(&data, &path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "0001020304050607");
        cleanup(&path);
    }

    #[test]
    fn test_export_exactly_9_bytes() {
        let data: Vec<u8> = (0..9).collect();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("hrush_export_9_test_{}", std::process::id()));

        export_hex_file(&data, &path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "0001020304050607 08");
        cleanup(&path);
    }

    #[test]
    fn test_roundtrip() {
        let original_data: Vec<u8> = (0..100).map(|i| (i % 256) as u8).collect();

        let dir = std::env::temp_dir();
        let path = dir.join(format!("hrush_roundtrip_test_{}", std::process::id()));

        // 导出
        export_hex_file(&original_data, &path).unwrap();

        // 重新导入
        let imported_data = parse_hex_file(&path).unwrap();

        assert_eq!(original_data, imported_data);
        cleanup(&path);
    }

    #[test]
    fn test_roundtrip_with_comments_and_empty_lines() {
        let content = "# Header comment\n\n0A0C1537AABBCCDD CCEEAABB01020304\n\n# Footer\n";
        let path = create_temp_file(content);
        let data = parse_hex_file(&path).unwrap();
        cleanup(&path);

        let dir = std::env::temp_dir();
        let out_path = dir.join(format!("hrush_roundtrip2_test_{}", std::process::id()));

        export_hex_file(&data, &out_path).unwrap();
        let reimported = parse_hex_file(&out_path).unwrap();

        assert_eq!(data, reimported);
        cleanup(&out_path);
    }
}


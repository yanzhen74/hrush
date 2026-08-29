//! 数据类型解读模块：将缓冲区中光标处的原始字节按多种类型解读为可读文本
//!
//! TYPE_DEFS 注册表及各解码函数由类型解读面板（ui/type_view.rs）消费。

/// 类型定义：表驱动注册表中的一项
pub struct TypeDef {
    pub label: &'static str,
    pub size: usize,
    pub needs_endian: bool,
    pub decode: fn(&[u8], bool) -> String,
}

/// 所有支持的数据类型注册表（顺序即面板显示顺序）
pub static TYPE_DEFS: &[TypeDef] = &[
    TypeDef { label: "u8", size: 1, needs_endian: false, decode: decode_u8 },
    TypeDef { label: "i8", size: 1, needs_endian: false, decode: decode_i8 },
    TypeDef { label: "u16", size: 2, needs_endian: true, decode: decode_u16 },
    TypeDef { label: "i16", size: 2, needs_endian: true, decode: decode_i16 },
    TypeDef { label: "u32", size: 4, needs_endian: true, decode: decode_u32 },
    TypeDef { label: "i32", size: 4, needs_endian: true, decode: decode_i32 },
    TypeDef { label: "u64", size: 8, needs_endian: true, decode: decode_u64 },
    TypeDef { label: "i64", size: 8, needs_endian: true, decode: decode_i64 },
    TypeDef { label: "f32", size: 4, needs_endian: true, decode: decode_f32 },
    TypeDef { label: "f64", size: 8, needs_endian: true, decode: decode_f64 },
    TypeDef { label: "str", size: 32, needs_endian: false, decode: decode_str },
    TypeDef { label: "hex", size: 16, needs_endian: false, decode: decode_hex },
];

/// 从 data 的 offset 处解读所有类型，返回 (标签, 值) 列表；
/// 字节不足时值为 "--"
pub fn interpret(data: &[u8], offset: usize, little_endian: bool) -> Vec<(&'static str, String)> {
    let data = &data[offset.min(data.len())..];
    TYPE_DEFS
        .iter()
        .map(|td| {
            let value = if data.len() < td.size {
                String::from("--")
            } else {
                // 单字节等类型不受端序影响，统一按 LE 解读
                let le = if td.needs_endian { little_endian } else { true };
                (td.decode)(&data[..td.size], le)
            };
            (td.label, value)
        })
        .collect()
}

fn pick<const N: usize>(bytes: &[u8], little_endian: bool) -> [u8; N] {
    let mut arr = [0u8; N];
    arr.copy_from_slice(&bytes[..N]);
    if little_endian {
        arr
    } else {
        arr.reverse();
        arr
    }
}

fn decode_u8(bytes: &[u8], _le: bool) -> String {
    bytes[0].to_string()
}

fn decode_i8(bytes: &[u8], _le: bool) -> String {
    (bytes[0] as i8).to_string()
}

fn decode_u16(bytes: &[u8], le: bool) -> String {
    u16::from_le_bytes(pick(bytes, le)).to_string()
}

fn decode_i16(bytes: &[u8], le: bool) -> String {
    i16::from_le_bytes(pick(bytes, le)).to_string()
}

fn decode_u32(bytes: &[u8], le: bool) -> String {
    u32::from_le_bytes(pick(bytes, le)).to_string()
}

fn decode_i32(bytes: &[u8], le: bool) -> String {
    i32::from_le_bytes(pick(bytes, le)).to_string()
}

fn decode_u64(bytes: &[u8], le: bool) -> String {
    u64::from_le_bytes(pick(bytes, le)).to_string()
}

fn decode_i64(bytes: &[u8], le: bool) -> String {
    i64::from_le_bytes(pick(bytes, le)).to_string()
}

fn decode_f32(bytes: &[u8], le: bool) -> String {
    f32::from_bits(u32::from_le_bytes(pick(bytes, le))).to_string()
}

fn decode_f64(bytes: &[u8], le: bool) -> String {
    f64::from_bits(u64::from_le_bytes(pick(bytes, le))).to_string()
}

/// UTF-8 字符串解读：最多 32 字节，截止到首个 NUL 或非法序列，
/// 控制字符转义为 \xNN
fn decode_str(bytes: &[u8], _le: bool) -> String {
    // 截止到首个 NUL
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let bytes = &bytes[..end];
    // 截止到首个非法 UTF-8 序列（取最长合法前缀）
    let mut valid_len = 0;
    for len in (1..=bytes.len()).rev() {
        if std::str::from_utf8(&bytes[..len]).is_ok() {
            valid_len = len;
            break;
        }
    }
    let s = std::str::from_utf8(&bytes[..valid_len]).unwrap_or("");
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_control() {
            for b in ch.to_string().as_bytes() {
                out.push_str(&format!("\\x{:02X}", b));
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// 16 字节大写十六进制，空格分隔
fn decode_hex(bytes: &[u8], _le: bool) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(rows: &'a [(&'static str, String)], label: &str) -> &'a str {
        rows.iter().find(|(l, _)| *l == label).map(|(_, v)| v.as_str()).unwrap()
    }

    #[test]
    fn integer_le_be() {
        // 0x01 0x02 0x03 0x04 ... 共 8 字节
        let data: [u8; 8] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];

        let le = interpret(&data, 0, true);
        assert_eq!(get(&le, "u8"), "1");
        assert_eq!(get(&le, "i8"), "1");
        assert_eq!(get(&le, "u16"), (0x0201u16).to_string());
        assert_eq!(get(&le, "u32"), (0x04030201u32).to_string());
        assert_eq!(get(&le, "u64"), u64::from_le_bytes(data).to_string());

        let be = interpret(&data, 0, false);
        assert_eq!(get(&be, "u16"), (0x0102u16).to_string());
        assert_eq!(get(&be, "u32"), (0x01020304u32).to_string());
        assert_eq!(get(&be, "u64"), u64::from_be_bytes(data).to_string());
    }

    #[test]
    fn signed_and_float() {
        let data: [u8; 8] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let rows = interpret(&data, 0, true);
        assert_eq!(get(&rows, "i8"), "-1");
        assert_eq!(get(&rows, "i16"), "-1");
        assert_eq!(get(&rows, "i32"), "-1");
        assert_eq!(get(&rows, "i64"), "-1");

        // f32: 1.5 的 IEEE-754 单精度表示（LE）为 00 00 C0 3F
        let data = [0x00u8, 0x00, 0xC0, 0x3F, 0, 0, 0, 0];
        let le = interpret(&data, 0, true);
        assert_eq!(get(&le, "f32"), "1.5");
        let be = interpret(&data, 0, false);
        assert_eq!(get(&be, "f32"), f32::from_bits(0x0000C03F).to_string());

        // f64: 1.5 的双精度表示（LE）为 00 00 00 00 00 00 F8 3F
        let data = [0x00u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF8, 0x3F];
        let le = interpret(&data, 0, true);
        assert_eq!(get(&le, "f64"), "1.5");
    }

    #[test]
    fn truncation_returns_dashes() {
        // 仅 3 字节：u8/i8/u16/i16 可解读，其余为 "--"
        let data = [0x01u8, 0x02, 0x03];
        let rows = interpret(&data, 0, true);
        assert_eq!(get(&rows, "u8"), "1");
        assert_eq!(get(&rows, "u16"), "513");
        assert_eq!(get(&rows, "u32"), "--");
        assert_eq!(get(&rows, "u64"), "--");
        assert_eq!(get(&rows, "f32"), "--");
        assert_eq!(get(&rows, "f64"), "--");
        assert_eq!(get(&rows, "str"), "--");
        assert_eq!(get(&rows, "hex"), "--");
    }

    #[test]
    fn empty_buffer_all_dashes() {
        let rows = interpret(&[], 0, true);
        assert_eq!(rows.len(), TYPE_DEFS.len());
        for (_, v) in &rows {
            assert_eq!(v, "--");
        }
    }

    #[test]
    fn offset_beyond_end_treated_as_empty() {
        let data = [0x01u8, 0x02];
        let rows = interpret(&data, 100, true);
        for (_, v) in &rows {
            assert_eq!(v, "--");
        }
    }

    #[test]
    fn str_stops_at_nul_and_escapes_control() {
        let mut data = vec![0u8; 32];
        data[..8].copy_from_slice(b"hi\x01\nab\x00Z");
        let rows = interpret(&data, 0, true);
        assert_eq!(get(&rows, "str"), "hi\\x01\\x0Aab");
    }

    #[test]
    fn str_stops_at_invalid_utf8() {
        let mut data = vec![b'A'; 32];
        data[2] = 0xFF; // 非法 UTF-8 起始字节
        data[3] = b'B';
        let rows = interpret(&data, 0, true);
        assert_eq!(get(&rows, "str"), "AA");
    }

    #[test]
    fn hex_uppercase_space_separated() {
        let data: Vec<u8> = (0..16u8).collect();
        let rows = interpret(&data, 0, true);
        assert_eq!(get(&rows, "hex"), "00 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F");
    }

    #[test]
    fn interpret_at_offset() {
        let data = [0xAAu8, 0x01, 0x02, 0x03, 0x04];
        let rows = interpret(&data, 1, true);
        assert_eq!(get(&rows, "u32"), (0x04030201u32).to_string());
    }
}

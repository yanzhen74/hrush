//! 校验和计算（CRC16 / CRC32 / MD5 / SHA256 / SUM8 / SUM16 / SUM32），均为同步纯计算，
//! 选区/全文范围由调用方决定（见 command::execute_command）。
//!
//! CRC 为通用表驱动引擎（运行时生成 256 项表），参数由 [`CrcParams`]
//! 描述；内置常用 16/32 位预设（见 [`crc16_preset`] / [`crc32_preset`]）。

use md5::Digest; // digest::Digest 由 md-5 crate re-export，sha2 通用

/// CRC 算法参数（标准 Rocksoft 模型，width 仅支持 16 / 32）
pub struct CrcParams {
    pub width: u32, // 16 or 32
    pub poly: u64,
    pub init: u64,
    pub refin: bool,
    pub refout: bool,
    pub xorout: u64,
}

// ---------- 内置预设 ----------

/// CRC16/CCITT-FALSE（`:crc16` 默认）
pub static CRC16_CCITT_FALSE: CrcParams = CrcParams {
    width: 16,
    poly: 0x1021,
    init: 0xFFFF,
    refin: false,
    refout: false,
    xorout: 0,
};

/// CRC16/XMODEM
pub static CRC16_XMODEM: CrcParams = CrcParams {
    width: 16,
    poly: 0x1021,
    init: 0,
    refin: false,
    refout: false,
    xorout: 0,
};

/// CRC16/MODBUS
pub static CRC16_MODBUS: CrcParams = CrcParams {
    width: 16,
    poly: 0x8005,
    init: 0xFFFF,
    refin: true,
    refout: true,
    xorout: 0,
};

/// CRC16/ARC
pub static CRC16_ARC: CrcParams = CrcParams {
    width: 16,
    poly: 0x8005,
    init: 0,
    refin: true,
    refout: true,
    xorout: 0,
};

/// CRC32/IEEE 802.3（`:crc32` 默认）
pub static CRC32_IEEE: CrcParams = CrcParams {
    width: 32,
    poly: 0x04C11DB7,
    init: 0xFFFF_FFFF,
    refin: true,
    refout: true,
    xorout: 0xFFFF_FFFF,
};

/// CRC32/C（Castagnoli）
pub static CRC32_C: CrcParams = CrcParams {
    width: 32,
    poly: 0x1EDC6F41,
    init: 0xFFFF_FFFF,
    refin: true,
    refout: true,
    xorout: 0xFFFF_FFFF,
};

/// CRC32/STM32（不反射，无输出异或）
pub static CRC32_STM32: CrcParams = CrcParams {
    width: 32,
    poly: 0x04C11DB7,
    init: 0xFFFF_FFFF,
    refin: false,
    refout: false,
    xorout: 0,
};

/// CRC16 预设查找（不区分大小写）：ccitt-false / xmodem / modbus / arc
pub fn crc16_preset(name: &str) -> Option<&'static CrcParams> {
    if name.eq_ignore_ascii_case("ccitt-false") {
        Some(&CRC16_CCITT_FALSE)
    } else if name.eq_ignore_ascii_case("xmodem") {
        Some(&CRC16_XMODEM)
    } else if name.eq_ignore_ascii_case("modbus") {
        Some(&CRC16_MODBUS)
    } else if name.eq_ignore_ascii_case("arc") {
        Some(&CRC16_ARC)
    } else {
        None
    }
}

/// CRC32 预设查找（不区分大小写）：ieee / c / stm32
pub fn crc32_preset(name: &str) -> Option<&'static CrcParams> {
    if name.eq_ignore_ascii_case("ieee") {
        Some(&CRC32_IEEE)
    } else if name.eq_ignore_ascii_case("c") {
        Some(&CRC32_C)
    } else if name.eq_ignore_ascii_case("stm32") {
        Some(&CRC32_STM32)
    } else {
        None
    }
}

/// 消息行/浮层展示用的预设名列表
pub fn crc16_preset_names() -> &'static str {
    "ccitt-false, xmodem, modbus, arc"
}

pub fn crc32_preset_names() -> &'static str {
    "ieee, c, stm32"
}

// ---------- 通用 CRC 引擎 ----------

/// 通用表驱动 CRC：运行时生成 256 项表，按字节处理。
/// refin 时对输入字节位反射；refout 时对最终结果位反射；最后与 xorout 异或，
/// 结果按 width 掩码。
pub fn crc(data: &[u8], p: &CrcParams) -> u64 {
    let mask = if p.width >= 64 {
        u64::MAX
    } else {
        (1u64 << p.width) - 1
    };
    let table = build_table(p, mask);

    let mut value = p.init & mask;
    for &byte in data {
        let byte = if p.refin { reflect_byte(byte) } else { byte };
        let index = ((value >> (p.width - 8)) ^ byte as u64) & 0xFF;
        value = ((value << 8) ^ table[index as usize]) & mask;
    }
    if p.refout {
        value = reflect_bits(value, p.width);
    }
    (value ^ p.xorout) & mask
}

/// 生成 256 项查找表（MSB-first，与 refin/refout 的外部反射模型配套）
fn build_table(p: &CrcParams, mask: u64) -> [u64; 256] {
    let top_bit = 1u64 << (p.width - 1);
    let mut table = [0u64; 256];
    for (i, entry) in table.iter_mut().enumerate() {
        let mut value = (i as u64) << (p.width - 8);
        for _ in 0..8 {
            value = if value & top_bit != 0 {
                (value << 1) ^ p.poly
            } else {
                value << 1
            };
        }
        *entry = value & mask;
    }
    table
}

/// 8 位位反射
fn reflect_byte(byte: u8) -> u8 {
    let mut result = 0u8;
    for i in 0..8 {
        if byte & (1 << i) != 0 {
            result |= 1 << (7 - i);
        }
    }
    result
}

/// 任意宽度位反射
fn reflect_bits(value: u64, width: u32) -> u64 {
    let mut result = 0u64;
    for i in 0..width {
        if value & (1u64 << i) != 0 {
            result |= 1u64 << (width - 1 - i);
        }
    }
    result
}

/// CRC16（默认预设 CCITT-FALSE）
pub fn crc16(data: &[u8]) -> u16 {
    crc(data, &CRC16_CCITT_FALSE) as u16
}

/// CRC32（默认预设 IEEE 802.3）
pub fn crc32(data: &[u8]) -> u32 {
    crc(data, &CRC32_IEEE) as u32
}

/// MD5，返回 32 位小写 hex
pub fn md5(data: &[u8]) -> String {
    let mut hasher = md5::Md5::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// SHA-256，返回 64 位小写 hex
pub fn sha256(data: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// 8 位累加和：所有字节累加，截断到 8 位
pub fn sum8(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b))
}

/// 16 位累加和：按 2 字节字累加，截断到 16 位；
/// 尾部不足 2 字节时按端序补 0（低位补 0 或高位补 0）
pub fn sum16(data: &[u8], little_endian: bool) -> u16 {
    let mut total = 0u16;
    for chunk in data.chunks(2) {
        let tail = chunk.get(1).copied().unwrap_or(0) as u16;
        let word = if little_endian {
            chunk[0] as u16 | (tail << 8)
        } else {
            ((chunk[0] as u16) << 8) | tail
        };
        total = total.wrapping_add(word);
    }
    total
}

/// 32 位累加和：按 4 字节字累加，截断到 32 位；尾部不足 4 字节补 0
pub fn sum32(data: &[u8], little_endian: bool) -> u32 {
    let mut total = 0u32;
    for chunk in data.chunks(4) {
        let mut word = 0u32;
        for (i, &b) in chunk.iter().enumerate() {
            let shift = if little_endian { i * 8 } else { (3 - i) * 8 };
            word |= (b as u32) << shift;
        }
        total = total.wrapping_add(word);
    }
    total
}

/// 校验和浮层展示所需的结果快照（打开 :sum 时一次性计算）。
/// CRC16 行固定使用 CCITT-FALSE，CRC32 行固定使用 IEEE；
/// SUM16/SUM32 按计算时的全局端序（sum_le）取字。
pub struct ChecksumInfo {
    /// 计算范围（含两端），字节数见 len
    pub range: (usize, usize),
    pub len: usize,
    pub crc16: String,
    pub crc32: String,
    pub md5: String,
    pub sha256: String,
    pub sum8: String,
    pub sum16: String,
    pub sum32: String,
    /// SUM16/SUM32 计算时使用的端序（true = LE）
    pub sum_le: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHECK_INPUT: &[u8] = b"123456789";

    /// 已知向量："abc"
    #[test]
    fn known_vectors_for_abc() {
        assert_eq!(crc32(b"abc"), 0x352441C2);
        assert_eq!(md5(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// 空输入向量
    #[test]
    fn known_vectors_for_empty_input() {
        assert_eq!(crc32(b""), 0x00000000);
        assert_eq!(md5(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(
            sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// CRC16 全部预设在 "123456789" 上的已知校验值
    #[test]
    fn crc16_known_vectors() {
        assert_eq!(crc(CHECK_INPUT, &CRC16_CCITT_FALSE), 0x29B1);
        assert_eq!(crc(CHECK_INPUT, &CRC16_XMODEM), 0x31C3);
        assert_eq!(crc(CHECK_INPUT, &CRC16_MODBUS), 0x4B37);
        assert_eq!(crc(CHECK_INPUT, &CRC16_ARC), 0xBB3D);
    }

    /// CRC32 全部预设在 "123456789" 上的已知校验值（STM32 见自洽性测试）
    #[test]
    fn crc32_known_vectors() {
        assert_eq!(crc(CHECK_INPUT, &CRC32_IEEE), 0xCBF43926);
        assert_eq!(crc(CHECK_INPUT, &CRC32_C), 0xE3069283);
    }

    /// CRC32/STM32 自洽性：表驱动实现与手动逐位实现结果一致
    /// （不硬编码外部值；手动实现按定义逐位推进，两种独立路径互为校验）
    #[test]
    fn crc32_stm32_matches_bitwise_reference() {
        fn crc32_bitwise(data: &[u8]) -> u32 {
            let mut crc = 0xFFFF_FFFFu32;
            for &byte in data {
                crc ^= (byte as u32) << 24;
                for _ in 0..8 {
                    crc = if crc & 0x8000_0000 != 0 {
                        (crc << 1) ^ 0x04C1_1DB7
                    } else {
                        crc << 1
                    };
                }
            }
            crc
        }
        assert_eq!(
            crc(CHECK_INPUT, &CRC32_STM32) as u32,
            crc32_bitwise(CHECK_INPUT)
        );
        assert_eq!(crc(b"", &CRC32_STM32) as u32, crc32_bitwise(b""));
    }

    /// 预设查找：不区分大小写；未知名称返回 None
    #[test]
    fn preset_lookup_is_case_insensitive() {
        assert!(crc16_preset("CCITT-FALSE").is_some());
        assert!(crc16_preset("ccitt-false").is_some());
        assert!(crc16_preset("Xmodem").is_some());
        assert!(crc16_preset("MODBUS").is_some());
        assert!(crc16_preset("arc").is_some());
        assert!(crc16_preset("nope").is_none());

        assert!(crc32_preset("IEEE").is_some());
        assert!(crc32_preset("c").is_some());
        assert!(crc32_preset("STM32").is_some());
        assert!(crc32_preset("nope").is_none());
    }

    /// 空输入：所有预设不 panic（此时结果仅由 init 经反射与 xorout 决定）
    #[test]
    fn empty_input_does_not_panic() {
        assert_eq!(crc(b"", &CRC16_CCITT_FALSE), 0xFFFF);
        assert_eq!(crc(b"", &CRC16_XMODEM), 0x0000);
        assert_eq!(crc(b"", &CRC16_MODBUS), 0xFFFF);
        assert_eq!(crc(b"", &CRC16_ARC), 0x0000);
        assert_eq!(crc(b"", &CRC32_IEEE), 0x0000_0000);
        assert_eq!(crc(b"", &CRC32_C), 0x0000_0000);
        assert_eq!(crc(b"", &CRC32_STM32), 0xFFFF_FFFF);
    }

    /// 累加和已知向量：[0x01, 0x02, 0x03]；尾部不足按端序补 0 参与累加
    #[test]
    fn sum_known_vectors() {
        let data = [0x01u8, 0x02, 0x03];
        assert_eq!(sum8(&data), 0x06);
        // LE: 0x0201 + 0x0003；BE: 0x0102 + 0x0300
        assert_eq!(sum16(&data, true), 0x0204);
        assert_eq!(sum16(&data, false), 0x0402);
        // 尾部不足补 0（与 sum16 语义一致）：LE: 0x00030201；BE: 0x01020300
        assert_eq!(sum32(&data, true), 0x0003_0201);
        assert_eq!(sum32(&data, false), 0x0102_0300);
    }

    /// 累加和溢出回绕（8/16/32 位截断）
    #[test]
    fn sum_wraps_on_overflow() {
        assert_eq!(sum8(&[0xFF, 0x02]), 0x01);
        assert_eq!(sum16(&[0xFF, 0xFF, 0x00, 0x01], true), 0x00FF); // 0xFFFF + 0x0100 回绕
        assert_eq!(
            sum32(&[0xFF; 4], true),
            0xFFFF_FFFF,
        );
        assert_eq!(sum32(&[0xFF, 0xFF, 0xFF, 0xFF, 0x01], true), 0x0000_0000);
    }

    /// 空输入：累加和为 0，不 panic
    #[test]
    fn sum_empty_input_is_zero() {
        assert_eq!(sum8(b""), 0);
        assert_eq!(sum16(b"", true), 0);
        assert_eq!(sum16(b"", false), 0);
        assert_eq!(sum32(b"", true), 0);
        assert_eq!(sum32(b"", false), 0);
    }
}

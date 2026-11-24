//! Registers, memory and disassembly of a stopped native debug session (lldb-dap; `SPEC-0054`
//! v6 behaviour 11): DAP `readMemory`, `disassemble` and the register scope of a frame. Reading
//! only; every amount has a limit and every address is hexadecimal.

use serde::Serialize;
use serde_json::{Value, json};

use crate::{DapError, DebugSession};

/// The most bytes one read of memory returns.
pub const MAX_MEMORY_BYTES: u64 = 4096;
/// The bytes read when no count is given.
pub const DEFAULT_MEMORY_BYTES: u64 = 256;
/// The most instructions one disassembly returns.
pub const MAX_INSTRUCTIONS: u64 = 256;
/// The instructions disassembled when no count is given.
pub const DEFAULT_INSTRUCTIONS: u64 = 32;
/// The most registers listed; a group beyond it is cut and says so.
pub const MAX_REGISTERS: usize = 512;
/// Bytes per row of a memory dump.
const ROW: usize = 16;

/// An address written by an agent: hexadecimal with `0x`, at most 16 digits.
pub fn parse_address(text: &str) -> Result<u64, String> {
    let digits = text
        .trim()
        .strip_prefix("0x")
        .or_else(|| text.trim().strip_prefix("0X"))
        .ok_or_else(|| {
            format!("address `{text}` is not hexadecimal with 0x, e.g. 0x7ffeefbff5a0")
        })?;
    if digits.is_empty() || digits.len() > 16 {
        return Err(format!("address `{text}` needs 1 to 16 hexadecimal digits"));
    }
    u64::from_str_radix(digits, 16).map_err(|_| format!("address `{text}` is not hexadecimal"))
}

/// An address as Ferris IDEA writes it: `0x` and 16 lowercase hexadecimal digits.
pub fn hex_address(address: u64) -> String {
    format!("0x{address:016x}")
}

/// An address an adapter gave (`0x…` or decimal), written as [`hex_address`] does; text that is
/// no address stays as it is.
pub fn normal_address(text: &str) -> String {
    parse_address(text)
        .ok()
        .or_else(|| text.trim().parse::<u64>().ok())
        .map_or_else(|| text.to_owned(), hex_address)
}

/// A count asked for, or the default, capped at the limit; zero is refused.
pub fn capped(asked: Option<u64>, default: u64, max: u64, what: &str) -> Result<u64, String> {
    match asked {
        Some(0) => Err(format!("{what} must be at least 1")),
        Some(n) => Ok(n.min(max)),
        None => Ok(default),
    }
}

/// Decodes standard base64 (DAP `readMemory` data), padding optional.
pub fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let clean: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let body = clean
        .iter()
        .rposition(|&b| b != b'=')
        .map_or(&clean[..0], |end| &clean[..=end]);
    if clean.len() - body.len() > 2 || body.len() % 4 == 1 {
        return Err("the adapter's memory data is not base64".into());
    }
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    for chunk in body.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            let v = value(c).ok_or("the adapter's memory data is not base64")?;
            acc |= v << (18 - 6 * i);
        }
        let bytes = acc.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Ok(out)
}

/// One row of a memory dump: its address, the bytes in hexadecimal and as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryRow {
    /// Address of the first byte.
    pub address: String,
    /// The bytes, two hexadecimal digits each, separated by spaces.
    pub hex: String,
    /// The bytes as ASCII, `.` for what is not printable.
    pub ascii: String,
}

/// The rows of `data` read from `start`, 16 bytes each.
pub fn memory_rows(start: u64, data: &[u8]) -> Vec<MemoryRow> {
    data.chunks(ROW)
        .enumerate()
        .map(|(i, chunk)| MemoryRow {
            address: hex_address(start.wrapping_add((i * ROW) as u64)),
            hex: chunk
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" "),
            ascii: chunk
                .iter()
                .map(|&b| {
                    if b.is_ascii_graphic() || b == b' ' {
                        char::from(b)
                    } else {
                        '.'
                    }
                })
                .collect(),
        })
        .collect()
}

/// What one read of memory gives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryBlock {
    /// Address of the first byte read.
    pub address: String,
    /// Bytes asked for after the limit.
    pub requested: u64,
    /// Bytes read.
    pub read: usize,
    /// Bytes the adapter could not read (DAP `unreadableBytes`), after those read.
    pub unreadable: u64,
    /// The count asked for was above the limit and was cut to it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub capped: bool,
    /// The bytes, 16 per row.
    pub rows: Vec<MemoryRow>,
}

/// The block in a `readMemory` answer for a read at `asked` of `requested` bytes.
pub fn memory_block(
    body: &Value,
    asked: u64,
    requested: u64,
    capped: bool,
) -> Result<MemoryBlock, String> {
    let start = body["address"]
        .as_str()
        .map_or(Ok(asked), parse_address)
        .unwrap_or(asked);
    let data = match body["data"].as_str() {
        Some(text) => decode_base64(text)?,
        None => Vec::new(),
    };
    Ok(MemoryBlock {
        address: hex_address(start),
        requested,
        read: data.len(),
        unreadable: body["unreadableBytes"].as_u64().unwrap_or_default(),
        capped,
        rows: memory_rows(start, &data),
    })
}

/// One disassembled instruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Instruction {
    /// Its address.
    pub address: String,
    /// Its bytes in hexadecimal, as the adapter gives them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<String>,
    /// The instruction text.
    pub instruction: String,
    /// The symbol it belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// The source file of the instruction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Its line in that file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<i64>,
}

/// The instructions of a `disassemble` answer, at most `max`. The text loses the blanks lldb-dap pads
/// it with to align its columns.
pub fn instructions(body: &Value, max: usize) -> Vec<Instruction> {
    let text = |v: &Value| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    body["instructions"]
        .as_array()
        .into_iter()
        .flatten()
        .take(max)
        .map(|i| Instruction {
            address: normal_address(i["address"].as_str().unwrap_or_default()),
            bytes: text(&i["instructionBytes"]),
            instruction: i["instruction"]
                .as_str()
                .unwrap_or_default()
                .trim_end()
                .to_owned(),
            symbol: text(&i["symbol"]),
            path: text(&i["location"]["path"]),
            line: i["line"].as_i64(),
        })
        .collect()
}

/// One register.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Register {
    /// Its name (`rip`, `x0`).
    pub name: String,
    /// Its value as the adapter shows it.
    pub value: String,
}

/// A group of registers (lldb-dap: General Purpose Registers, Floating Point Registers, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegisterGroup {
    /// The group's name.
    pub name: String,
    /// Its registers, in the adapter's order.
    pub registers: Vec<Register>,
    /// Registers left out after [`MAX_REGISTERS`].
    #[serde(skip_serializing_if = "is_zero")]
    pub left_out: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// The scope of a frame that holds its registers: the one named for them (lldb-dap
/// `Registers`), else none.
pub fn register_scope(scopes: &[(String, i64)]) -> Option<i64> {
    scopes
        .iter()
        .find(|(name, r)| *r != 0 && name.to_ascii_lowercase().contains("register"))
        .map(|(_, r)| *r)
}

/// Why a session cannot show what an agent asked for: not stopped, or no native adapter feature.
pub const NOT_STOPPED: &str =
    "the session is not stopped: registers, memory and disassembly are read only while it is";

impl DebugSession {
    /// Reads `count` bytes (at most [`MAX_MEMORY_BYTES`], default [`DEFAULT_MEMORY_BYTES`]) at
    /// `address` with DAP `readMemory`; refused when the adapter cannot.
    pub fn read_memory(
        &mut self,
        address: u64,
        count: Option<u64>,
    ) -> Result<MemoryBlock, DapError> {
        crate::session::supported(
            self.capabilities(),
            "supportsReadMemoryRequest",
            "readMemory",
            "The debug adapter cannot read memory",
        )?;
        let requested = capped(count, DEFAULT_MEMORY_BYTES, MAX_MEMORY_BYTES, "count")
            .map_err(failed("readMemory"))?;
        let body = self.request(
            "readMemory",
            json!({"memoryReference": hex_address(address), "offset": 0, "count": requested}),
        )?;
        memory_block(
            &body,
            address,
            requested,
            count.is_some_and(|c| c > MAX_MEMORY_BYTES),
        )
        .map_err(failed("readMemory"))
    }

    /// Disassembles `count` instructions (at most [`MAX_INSTRUCTIONS`], default
    /// [`DEFAULT_INSTRUCTIONS`]) starting `before` instructions ahead of `address`, with symbols;
    /// refused when the adapter cannot.
    pub fn disassemble(
        &mut self,
        address: u64,
        before: u64,
        count: Option<u64>,
    ) -> Result<Vec<Instruction>, DapError> {
        crate::session::supported(
            self.capabilities(),
            "supportsDisassembleRequest",
            "disassemble",
            "The debug adapter cannot disassemble",
        )?;
        let count = capped(count, DEFAULT_INSTRUCTIONS, MAX_INSTRUCTIONS, "count")
            .map_err(failed("disassemble"))?;
        let before = before.min(MAX_INSTRUCTIONS);
        let body = self.request(
            "disassemble",
            json!({
                "memoryReference": hex_address(address),
                "offset": 0,
                "instructionOffset": -(before as i64),
                "instructionCount": count,
                "resolveSymbols": true,
            }),
        )?;
        Ok(instructions(&body, count as usize))
    }

    /// The address the top frame of `thread_id` stands at (DAP `instructionPointerReference`).
    pub fn instruction_pointer(&mut self, thread_id: i64) -> Result<u64, DapError> {
        let body = self.request(
            "stackTrace",
            json!({"threadId": thread_id, "startFrame": 0, "levels": 1}),
        )?;
        body["stackFrames"][0]["instructionPointerReference"]
            .as_str()
            .and_then(|r| parse_address(r).ok())
            .ok_or_else(|| DapError::Failed {
                command: "stackTrace".into(),
                message: "The debug adapter gives no instruction address for this frame".into(),
            })
    }

    /// The registers of the top frame of `thread_id` by group, each group cut at
    /// [`MAX_REGISTERS`]; refused when the frame has no register scope.
    pub fn registers(&mut self, thread_id: i64) -> Result<Vec<RegisterGroup>, DapError> {
        let frame = self
            .stack_trace(thread_id)?
            .first()
            .map(|f| f.id)
            .ok_or_else(|| DapError::Failed {
                command: "stackTrace".into(),
                message: "The stopped thread has no frame".into(),
            })?;
        let scopes = self.scopes(frame)?;
        let reference = register_scope(&scopes).ok_or_else(|| DapError::Failed {
            command: "scopes".into(),
            message: "The debug adapter shows no registers for this frame".into(),
        })?;
        let mut groups = Vec::new();
        let mut loose = Vec::new();
        for child in self.variables(reference)? {
            if child.reference == 0 {
                loose.push(Register {
                    name: child.name,
                    value: child.value,
                });
                continue;
            }
            let all: Vec<Register> = self
                .variables(child.reference)?
                .into_iter()
                .map(|v| Register {
                    name: v.name,
                    value: v.value,
                })
                .collect();
            groups.push(group(child.name, all));
        }
        if !loose.is_empty() {
            groups.insert(0, group("Registers".into(), loose));
        }
        Ok(groups)
    }
}

fn group(name: String, mut registers: Vec<Register>) -> RegisterGroup {
    let left_out = registers.len().saturating_sub(MAX_REGISTERS);
    registers.truncate(MAX_REGISTERS);
    RegisterGroup {
        name,
        registers,
        left_out,
    }
}

fn failed(command: &'static str) -> impl Fn(String) -> DapError {
    move |message| DapError::Failed {
        command: command.into(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0054 v6 AK-6: addresses are hexadecimal with 0x, written with 16 digits.
    #[test]
    fn addresses_are_hexadecimal() {
        assert_eq!(parse_address("0x7ffeefbff5a0"), Ok(0x7ffe_efbf_f5a0));
        assert_eq!(parse_address(" 0XFF "), Ok(255));
        assert!(
            parse_address("140737488348576").is_err(),
            "decimal is refused"
        );
        assert!(parse_address("0x").is_err());
        assert!(parse_address("0x1ffffffffffffffff").is_err(), "17 digits");
        assert!(parse_address("0xzz").is_err());
        assert_eq!(hex_address(0x1000), "0x0000000000001000");
        assert_eq!(normal_address("0x1000"), "0x0000000000001000");
        assert_eq!(normal_address("4096"), "0x0000000000001000");
        assert_eq!(normal_address("main+4"), "main+4");
    }

    /// SPEC-0054 v6 AK-6: counts have a default and a limit; zero is refused.
    #[test]
    fn counts_are_capped() {
        assert_eq!(capped(None, 256, 4096, "count"), Ok(256));
        assert_eq!(capped(Some(10_000), 256, 4096, "count"), Ok(4096));
        assert!(capped(Some(0), 256, 4096, "count").is_err());
    }

    #[test]
    fn base64_is_decoded() {
        assert_eq!(decode_base64("SGVsbG8=").unwrap(), b"Hello");
        assert_eq!(decode_base64("SGVsbG8").unwrap(), b"Hello");
        assert_eq!(decode_base64("AAEC/w==").unwrap(), [0, 1, 2, 255]);
        assert_eq!(decode_base64("").unwrap(), Vec::<u8>::new());
        assert!(decode_base64("S").is_err());
        assert!(decode_base64("SG!s").is_err());
    }

    /// SPEC-0054 v6 AK-6: a fixed `readMemory` answer as rows of 16 bytes with text.
    #[test]
    fn memory_answers_become_rows() {
        // "ABCDEFGHIJKLMNOPQR" in base64.
        let body =
            json!({"address": "0x1000", "data": "QUJDREVGR0hJSktMTU5PUFFS", "unreadableBytes": 14});
        let block = memory_block(&body, 0x1000, 32, false).unwrap();
        assert_eq!(block.read, 18);
        assert_eq!(block.unreadable, 14);
        assert_eq!(block.rows.len(), 2);
        assert_eq!(block.rows[0].address, "0x0000000000001000");
        assert_eq!(&block.rows[0].hex[..11], "41 42 43 44");
        assert_eq!(block.rows[0].ascii, "ABCDEFGHIJKLMNOP");
        assert_eq!(block.rows[1].address, "0x0000000000001010");
        assert_eq!(block.rows[1].ascii, "QR");
        assert_eq!(memory_rows(0, &[0, 0x7f, b' '])[0].ascii, ".. ");
        assert!(memory_block(&json!({"data": "!!"}), 0, 1, false).is_err());
    }

    /// SPEC-0054 v6 AK-6: a fixed `disassemble` answer with symbols and lines, cut at the count.
    #[test]
    fn disassembly_answers_become_instructions() {
        let body = json!({"instructions": [
            {"address": "0x100003f80", "instructionBytes": "55", "instruction": "pushq  %rbp",
             "symbol": "main", "location": {"path": "/src/main.c"}, "line": 3},
            {"address": "0x100003f81", "instruction": "movq   %rsp, %rbp       ", "symbol": ""},
            {"address": "0x100003f84", "instruction": "nop"},
        ]});
        let got = instructions(&body, 2);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].address, "0x0000000100003f80");
        assert_eq!(got[0].symbol.as_deref(), Some("main"));
        assert_eq!(got[0].line, Some(3));
        assert_eq!(got[1].symbol, None, "an empty symbol is left out");
        assert_eq!(
            serde_json::to_value(&got[1]).unwrap(),
            json!({"address": "0x0000000100003f81", "instruction": "movq   %rsp, %rbp"})
        );
    }

    #[test]
    fn the_register_scope_is_found_by_name() {
        let scopes = vec![
            ("Locals".to_owned(), 1),
            ("Globals".to_owned(), 2),
            ("Registers".to_owned(), 3),
        ];
        assert_eq!(register_scope(&scopes), Some(3));
        assert_eq!(register_scope(&scopes[..2]), None);
        let many: Vec<Register> = (0..MAX_REGISTERS + 3)
            .map(|i| Register {
                name: format!("r{i}"),
                value: "0".into(),
            })
            .collect();
        let g = group("General".into(), many);
        assert_eq!((g.registers.len(), g.left_out), (MAX_REGISTERS, 3));
    }
}

#!/usr/bin/env python3
"""Generate the pinned hUGEDriver relocation matcher from its RGBDS object."""

import argparse
import struct
from dataclasses import dataclass
from pathlib import Path


ROM0 = 3
WRAM0 = 0
PATCH_BYTE = 0
PATCH_WORD = 1
PATCH_LONG = 2
PATCH_JR = 3
RPN_ADD = 0x00
RPN_SUB = 0x01
RPN_HIGH = 0x70
RPN_LOW = 0x71
RPN_INT = 0x80
RPN_SYMBOL = 0x81


class Reader:
    def __init__(self, data: bytes) -> None:
        self.data = data
        self.at = 0

    def byte(self) -> int:
        value = self.data[self.at]
        self.at += 1
        return value

    def u32(self) -> int:
        value, = struct.unpack_from("<I", self.data, self.at)
        self.at += 4
        return value

    def i32(self) -> int:
        value, = struct.unpack_from("<i", self.data, self.at)
        self.at += 4
        return value

    def take(self, size: int) -> bytes:
        value = self.data[self.at:self.at + size]
        if len(value) != size:
            raise ValueError("truncated RGBDS object")
        self.at += size
        return value

    def string(self) -> str:
        try:
            end = self.data.index(0, self.at)
        except ValueError as error:
            raise ValueError("unterminated RGBDS string") from error
        value = self.data[self.at:end].decode("utf-8")
        self.at = end + 1
        return value


@dataclass(frozen=True)
class Symbol:
    name: str
    section: int | None
    value: int


@dataclass(frozen=True)
class Patch:
    offset: int
    pc_section: int
    pc_offset: int
    kind: int
    expression: bytes


@dataclass
class Section:
    name: str
    kind: int
    size: int
    data: bytes | None
    patches: list[Patch]


@dataclass(frozen=True)
class Value:
    domain: str | None
    offset: int
    byte: str | None = None


def read_object(path: Path) -> tuple[list[Symbol], list[Section]]:
    reader = Reader(path.read_bytes())
    if reader.take(4) != b"RGB9" or reader.u32() != 13:
        raise ValueError("expected RGBDS RGB9 revision 13")
    symbol_count, section_count = reader.u32(), reader.u32()
    for _ in range(reader.u32()):
        reader.u32()
        reader.u32()
        kind = reader.byte() & 0x7F
        if kind:
            reader.string()
        else:
            reader.take(reader.u32() * 4)
    symbols = []
    for _ in range(symbol_count):
        name, kind = reader.string(), reader.byte()
        if kind == 1:
            symbols.append(Symbol(name, None, 0))
        else:
            reader.u32()
            reader.u32()
            section = reader.i32()
            symbols.append(Symbol(name, None if section == -1 else section, reader.u32()))
    sections = []
    for _ in range(section_count):
        name = reader.string()
        reader.u32()
        reader.u32()
        size = reader.u32()
        kind = reader.byte()
        reader.i32()
        reader.i32()
        reader.byte()
        reader.u32()
        data = None
        patches = []
        if kind & 7 in (2, ROM0):
            data = reader.take(size)
            for _ in range(reader.u32()):
                reader.u32()
                reader.u32()
                offset = reader.u32()
                pc_section = reader.u32()
                pc_offset = reader.u32()
                patch_kind = reader.byte()
                patches.append(
                    Patch(
                        offset,
                        pc_section,
                        pc_offset,
                        patch_kind,
                        reader.take(reader.u32()),
                    )
                )
        sections.append(Section(name, kind & 7, size, data, patches))
    for _ in range(reader.u32()):
        reader.u32()
        reader.u32()
        reader.u32()
        reader.u32()
        reader.u32()
        reader.byte()
        reader.take(reader.u32())
        reader.string()
    if reader.at != len(reader.data):
        raise ValueError("unexpected RGBDS data after sections")
    return symbols, sections


def symbol_value(symbols: list[Symbol], sections: list[Section], symbol_id: int) -> Value:
    symbol = symbols[symbol_id]
    if symbol.section is None:
        raise ValueError(f"unsupported imported or constant symbol {symbol.name}")
    section = sections[symbol.section]
    if section.name == "Sound Driver" and section.kind == ROM0:
        return Value("code", symbol.value)
    if section.name == "Playback variables" and section.kind == WRAM0:
        return Value("ram", symbol.value)
    raise ValueError(f"unsupported symbol section for {symbol.name}")


def combine(left: Value, right: Value, operator: int) -> Value:
    if left.byte or right.byte:
        raise ValueError("operation after HIGH or LOW")
    if left.domain is None and right.domain is None:
        return Value(None, left.offset + right.offset if operator == RPN_ADD else left.offset - right.offset)
    if operator == RPN_ADD:
        if left.domain is not None and right.domain is None:
            return Value(left.domain, left.offset + right.offset)
        if left.domain is None and right.domain is not None:
            return Value(right.domain, left.offset + right.offset)
    elif left.domain is not None and right.domain is None:
        return Value(left.domain, left.offset - right.offset)
    elif left.domain == right.domain:
        return Value(None, left.offset - right.offset)
    raise ValueError("unsupported symbolic RGBDS expression")


def expression_value(expression: bytes, symbols: list[Symbol], sections: list[Section]) -> Value:
    reader = Reader(expression)
    stack: list[Value] = []
    while reader.at != len(expression):
        token = reader.byte()
        if token == RPN_INT:
            stack.append(Value(None, reader.i32()))
        elif token == RPN_SYMBOL:
            stack.append(symbol_value(symbols, sections, reader.u32()))
        elif token in (RPN_ADD, RPN_SUB):
            right, left = stack.pop(), stack.pop()
            stack.append(combine(left, right, token))
        elif token in (RPN_HIGH, RPN_LOW):
            value = stack.pop()
            if value.byte:
                raise ValueError("repeated HIGH or LOW")
            stack.append(Value(value.domain, value.offset, "high" if token == RPN_HIGH else "low"))
        else:
            raise ValueError(f"unsupported RGBDS RPN token {token:#x}")
    if len(stack) != 1:
        raise ValueError("malformed RGBDS RPN expression")
    return stack[0]


def materialize(value: Value, code_base: int, ram_base: int) -> int:
    base = {"code": code_base, "ram": ram_base}.get(value.domain, 0)
    raw = (base + value.offset) & 0xFFFFFFFF
    if value.byte == "low":
        return raw & 0xFF
    if value.byte == "high":
        return raw >> 8 & 0xFF
    return raw


def patch_bytes(patch: Patch, value: Value, code_base: int, ram_base: int) -> bytes:
    raw = materialize(value, code_base, ram_base)
    if patch.kind == PATCH_BYTE:
        return bytes([raw & 0xFF])
    if patch.kind == PATCH_WORD:
        return struct.pack("<H", raw & 0xFFFF)
    if patch.kind == PATCH_LONG:
        return struct.pack("<I", raw)
    if patch.kind == PATCH_JR:
        pc = code_base + patch.pc_offset
        return bytes([(raw - (pc + 2)) & 0xFF])
    raise ValueError(f"unsupported RGBDS patch kind {patch.kind}")


def relocations(patches: list[Patch], symbols: list[Symbol], sections: list[Section]) -> list[tuple[int, int, str]]:
    output = []
    code_section = next(index for index, section in enumerate(sections) if section.name == "Sound Driver")
    for patch in patches:
        value = expression_value(patch.expression, symbols, sections)
        if patch.kind == PATCH_JR:
            if value.domain != "code" or patch.pc_section != code_section:
                raise ValueError("JR patch is not code-local")
            continue
        if value.domain is None:
            continue
        if patch.kind == PATCH_WORD and value.byte is None:
            output.extend([
                (patch.offset, value.offset, f"{value.domain.title()}Low"),
                (patch.offset + 1, value.offset, f"{value.domain.title()}High"),
            ])
        elif patch.kind == PATCH_BYTE and value.byte is not None:
            output.append((patch.offset, value.offset, f"{value.domain.title()}{value.byte.title()}"))
        else:
            raise ValueError("unsupported relocatable RGBDS patch")
    output.sort()
    if len({offset for offset, _, _ in output}) != len(output):
        raise ValueError("overlapping relocatable patches")
    return output


def render_bytes(data: bytes) -> str:
    rows = [", ".join(f"0x{byte:02x}" for byte in data[index:index + 16]) for index in range(0, len(data), 16)]
    return ",\n    ".join(rows)


def render_patches(items: list[tuple[int, int, str]]) -> str:
    kinds = {"CodeLow": 0, "CodeHigh": 1, "RamLow": 2, "RamHigh": 3}
    if any(at >= 1 << 11 or offset < 0 or offset >= 1 << 11 for at, offset, _ in items):
        raise ValueError("relocation cannot fit the generated packed fields")
    packed = [at | offset << 11 | kinds[kind] << 22 for at, offset, kind in items]
    rows = [", ".join(f"0x{item:06x}" for item in packed[index:index + 10]) for index in range(0, len(packed), 10)]
    return ",\n    ".join(rows)


def relocation_byte(kind: str, offset: int, code_base: int, ram_base: int) -> int:
    base = code_base if kind.startswith("Code") else ram_base
    value = (base + offset) & 0xFFFF
    return value & 0xFF if kind.endswith("Low") else value >> 8


def ram_anchor(items: list[tuple[int, int, str]]) -> tuple[int, int]:
    for (at, offset, kind), (next_at, next_offset, next_kind) in zip(items, items[1:]):
        if kind == "RamLow" and (next_at, next_offset, next_kind) == (at + 1, offset, "RamHigh"):
            return at, offset
    raise ValueError("no consecutive RAM word relocation for inference")


def generate(args: argparse.Namespace) -> None:
    symbols, sections = read_object(args.object)
    code_section = next(section for section in sections if section.name == "Sound Driver")
    if code_section.kind != ROM0 or code_section.size != 1941 or code_section.data is None:
        raise ValueError("expected a 1941-byte ROM0 Sound Driver section")
    rom = args.rom.read_bytes()
    code = rom[args.code_base:args.code_base + code_section.size]
    if len(code) != code_section.size:
        raise ValueError("linked ROM does not contain Sound Driver")
    materialized = bytearray(code_section.data)
    for patch in code_section.patches:
        value = expression_value(patch.expression, symbols, sections)
        expected = patch_bytes(patch, value, args.code_base, args.ram_base)
        materialized[patch.offset:patch.offset + len(expected)] = expected
        actual = code[patch.offset:patch.offset + len(expected)]
        if actual != expected:
            raise ValueError(f"linked patch mismatch at {patch.offset:#x}: {actual.hex()} != {expected.hex()}")
    if code != materialized:
        raise ValueError("linked ROM differs from the fully materialized object section")
    if next(symbol.value for symbol in symbols if symbol.name == "hUGE_init") != 0:
        raise ValueError("hUGE_init must start the Sound Driver section")
    update = next(symbol.value for symbol in symbols if symbol.name == "hUGE_dosound")
    if update != 0x469:
        raise ValueError("unexpected hUGE_dosound offset")
    ram = next(section.size for section in sections if section.name == "Playback variables" and section.kind == WRAM0)
    if ram != 100:
        raise ValueError("unexpected Playback variables size")
    patches = relocations(code_section.patches, symbols, sections)
    anchor_at, anchor_offset = ram_anchor(patches)
    if args.verify_rom:
        other = args.verify_rom.read_bytes()[args.verify_code_base:args.verify_code_base + len(code)]
        expected = bytearray(code)
        for at, offset, kind in patches:
            expected[at] = relocation_byte(kind, offset, args.verify_code_base, args.verify_ram_base)
        if other != expected:
            raise ValueError("relocated linked ROM does not match every generated driver byte")
        matching_ram = [
            ram_base
            for ram_base in range(0xC000, 0xD000)
            if all(other[at] == relocation_byte(kind, offset, 0, ram_base) for at, offset, kind in patches if kind.startswith("Ram"))
        ]
        if matching_ram != [args.verify_ram_base]:
            raise ValueError(f"RAM relocation is not uniquely inferred: {matching_ram}")
    text = f'''pub(super) const CODE: &[u8] = &[
    {render_bytes(code)},
];

const PATCHES: &[u32] = &[
    // Bits 0..10 are the code offset, 11..21 the relocation offset, and 22..23 its kind.
    {render_patches(patches)},
];

const RAM_ANCHOR_AT: usize = {anchor_at};
const RAM_ANCHOR_OFFSET: u16 = {anchor_offset};

pub(super) const UPDATE_OFFSET: usize = {update};
pub(super) const RAM_SIZE: usize = {ram};

pub(super) fn relocated_byte(index: usize, code_base: u16, ram_base: u16) -> u8 {{
    match patch_for(index) {{
        Some(patch) => patch_byte(patch, code_base, ram_base),
        None => CODE[index],
    }}
}}

pub(super) fn infer_ram(bytes: &[u8]) -> Option<u16> {{
    if bytes.len() != CODE.len() {{
        return None;
    }}
    let value = u16::from_le_bytes([bytes[RAM_ANCHOR_AT], bytes[RAM_ANCHOR_AT + 1]]);
    let ram = value.checked_sub(RAM_ANCHOR_OFFSET)?;
    (0xc000..=0xcfff).contains(&ram).then_some(ram)
}}

fn patch_for(index: usize) -> Option<u32> {{
    let at = u16::try_from(index).ok()?;
    PATCHES
        .binary_search_by_key(&at, |patch| (*patch & 0x7ff) as u16)
        .ok()
        .map(|index| PATCHES[index])
}}

fn patch_byte(patch: u32, code_base: u16, ram_base: u16) -> u8 {{
    let kind = patch >> 22;
    let base = if kind < 2 {{ code_base }} else {{ ram_base }};
    let value = base.wrapping_add(((patch >> 11) & 0x7ff) as u16);
    if kind & 1 == 0 {{ value as u8 }} else {{ (value >> 8) as u8 }}
}}

'''
    args.output.write_text(text, newline="\n")
    print(f"generated {args.output}: {len(code)} bytes, {len(patches)} relocated bytes")


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser()
    parser.add_argument("--object", type=Path, required=True)
    parser.add_argument("--rom", type=Path, required=True)
    parser.add_argument("--code-base", type=lambda text: int(text, 0), default=0x200)
    parser.add_argument("--ram-base", type=lambda text: int(text, 0), default=0xc000)
    parser.add_argument("--output", type=Path, default=root / "crates/zeff-audio-discovery/src/huge/discovery/reference.rs")
    parser.add_argument("--verify-rom", type=Path)
    parser.add_argument("--verify-code-base", type=lambda text: int(text, 0), default=0x800)
    parser.add_argument("--verify-ram-base", type=lambda text: int(text, 0), default=0xc200)
    generate(parser.parse_args())


if __name__ == "__main__":
    main()

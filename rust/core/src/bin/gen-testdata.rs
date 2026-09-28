//! Deterministic test data generator for the prototype benchmarks (handoff §7.2).
//!
//! `cargo run --release -p tc-core --bin gen-testdata -- ../testdata` (path relative to `rust/`).
//! Uses an inline splitmix64 PRNG with fixed seeds so every run produces byte-identical files.

use std::env;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

const BIG_ROWS: u64 = 16_000_000;

struct Rng(u64);

impl Rng {
    const fn new(seed: u64) -> Rng {
        Rng(seed)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.below(hi - lo + 1)
    }
    fn pick<'a>(&mut self, xs: &'a [&'a str]) -> &'a str {
        xs[self.below(xs.len() as u64) as usize]
    }
}

const FIRST: &[&str] = &[
    "Ann", "Ben", "Cara", "Dan", "Eva", "Finn", "Gia", "Hugo", "Ivy", "Jack", "Kim", "Liam",
    "Mona", "Noah", "Omar", "Pia", "Quinn", "Rosa", "Sam", "Tara", "Uma", "Vik", "Wren", "Zoe",
    "Jose", "Lars", "Zoë", "José",
];
const LAST: &[&str] = &[
    "Smith",
    "Jones",
    "Brown",
    "Davis",
    "Miller",
    "Wilson",
    "Moore",
    "Taylor",
    "Clark",
    "Hall",
    "Young",
    "King",
    "Lee",
    "Scott",
    "Green",
    "Adams",
    "Baker",
    "Nelson",
    "Carter",
    "Müller",
    "García",
    "Sørensen",
];
const CITY: &[&str] = &[
    "Berlin",
    "Paris",
    "Tokyo",
    "Madrid",
    "Cairo",
    "Oslo",
    "Lima",
    "Rome",
    "Bern",
    "Austin",
    "Denver",
    "Lisbon",
    "Toronto",
    "Osaka",
    "São Paulo",
    "München",
    "Zürich",
];
const CATEGORY: &[&str] = &[
    "home", "auto", "tech", "food", "trip", "misc", "garden", "sports", "books", "toys", "health",
    "office",
];
const NOTES_WORDS: &[&str] = &[
    "delivery", "invoice", "pending", "review", "urgent", "backup", "archive", "refund", "retry",
    "urgent", "sample", "batch", "region", "season", "manual", "auto", "pilot", "draft", "final",
    "check", "audit", "trace", "stack", "range",
];

fn push_field(out: &mut String, field: &str, delim: char) {
    if field.contains(delim) || field.contains('"') || field.contains('\n') || field.contains('\r')
    {
        out.push('"');
        for ch in field.chars() {
            if ch == '"' {
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
    } else {
        out.push_str(field);
    }
}

fn make_notes(rng: &mut Rng, i: u64, needle: Option<&str>) -> String {
    let mut extras = String::new();
    if i.is_multiple_of(1000) {
        extras.push_str("\ncontinued");
    }
    if i.is_multiple_of(50) {
        extras.push_str(" \"quoted\"");
    }
    if i.is_multiple_of(20) {
        extras.push_str(", extra");
    }
    let target = rng.range(20, (60usize).saturating_sub(extras.len()).max(20) as u64) as usize;
    let mut s = String::new();
    while s.len() < target {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(rng.pick(NOTES_WORDS));
    }
    while s.len() > 60 {
        match s.rfind(' ') {
            Some(p) => s.truncate(p),
            None => break,
        }
    }
    if let Some(needle) = needle {
        let mut with = String::from(needle);
        while with.len() < 20 {
            with.push(' ');
            with.push_str(rng.pick(NOTES_WORDS));
        }
        s = with;
    }
    s.push_str(&extras);
    s
}

fn date(rng: &mut Rng) -> String {
    let year = rng.range(2020, 2026);
    let month = rng.range(1, 12);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        2 => 28,
        _ => 30,
    };
    let day = rng.range(1, max_day);
    format!("{:04}-{:02}-{:02}", year, month, day)
}

fn amount(rng: &mut Rng) -> String {
    let sign = if rng.below(4) == 0 { "-" } else { "" };
    format!("{}{}.{:02}", sign, rng.range(0, 9999), rng.range(0, 99))
}

fn create(path: &Path) -> io::Result<BufWriter<File>> {
    Ok(BufWriter::with_capacity(4 << 20, File::create(path)?))
}

fn gen_big(rng: &mut Rng, path: &Path) -> io::Result<()> {
    let mut w = create(path)?;
    w.write_all(b"id,first_name,last_name,email,city,amount,date,category,notes,flag\n")?;
    let mut row = String::with_capacity(256);
    for i in 1..=BIG_ROWS {
        row.clear();
        if i % 1_000_000 == 0 {
            eprintln!("  big.csv: {} rows", i);
        }
        let first = rng.pick(FIRST);
        let last = rng.pick(LAST);
        let id = i.wrapping_mul(2654435761) & 0xFFFF_FFFF;
        let email = format!("{}{}{}@example.com", first, last, rng.range(1, 99));
        let notes = make_notes(
            rng,
            i,
            if i == BIG_ROWS {
                Some("NEEDLE_7f3a")
            } else {
                None
            },
        );
        let flag = if rng.below(10) == 0 { "1" } else { "0" };
        let _ = write!(
            row,
            "{},{},{},{},{},{},{},{},",
            id,
            first,
            last,
            email,
            rng.pick(CITY),
            amount(rng),
            date(rng),
            rng.pick(CATEGORY)
        );
        push_field(&mut row, &notes, ',');
        row.push(',');
        row.push_str(flag);
        row.push('\n');
        w.write_all(row.as_bytes())?;
    }
    w.flush()
}

fn gen_wide(rng: &mut Rng, path: &Path) -> io::Result<()> {
    const ROWS: u64 = 100_000;
    const COLS: u64 = 500;
    let mut w = create(path)?;
    let mut header = String::new();
    for c in 0..COLS {
        if c > 0 {
            header.push(',');
        }
        let _ = write!(header, "c{}", c);
    }
    header.push('\n');
    w.write_all(header.as_bytes())?;
    let mut row = String::with_capacity(COLS as usize * 4);
    for _ in 0..ROWS {
        row.clear();
        for c in 0..COLS {
            if c > 0 {
                row.push(',');
            }
            let _ = write!(row, "{}", rng.range(0, 99));
        }
        row.push('\n');
        w.write_all(row.as_bytes())?;
    }
    w.flush()
}

fn gen_quoted(rng: &mut Rng, path: &Path) -> io::Result<()> {
    const ROWS: u64 = 1_000_000;
    let mut w = create(path)?;
    w.write_all(b"id,name,value,note,flag,text\r\n")?;
    let mut row = String::with_capacity(128);
    for i in 1..=ROWS {
        if i % 200_000 == 0 {
            eprintln!("  quoted.csv: {} rows", i);
        }
        row.clear();
        let first = rng.pick(FIRST);
        let last = rng.pick(LAST);
        let name = match i % 7 {
            0 => format!("\"{}, {}\"", last, first),
            1 => format!("\"{} \"\"Q\"\" {}\"", first, last),
            2 => format!("{}\"{}", first, last),
            3 => format!("\"{}\"{}", first, last),
            4 => format!("{}-{}", first, last),
            5 => "\"\"".to_string(),
            _ => format!("\"{} {}\"", first, last),
        };
        let note = match i % 5 {
            0 => "\"line1\r\nline2\"".to_string(),
            1 => "\"has, comma\"".to_string(),
            2 => "\"quote \"\" inside\"".to_string(),
            3 => "plain".to_string(),
            _ => "\"a\"\"b\"".to_string(),
        };
        let text = if i == ROWS {
            "NEEDLE_7f3a".to_string()
        } else {
            rng.pick(NOTES_WORDS).to_string()
        };
        let _ = write!(
            row,
            "{},{},{:.2},{},{},{}",
            i,
            name,
            rng.range(0, 99999) as f64 / 100.0 - 500.0,
            note,
            if rng.below(2) == 0 { "0" } else { "1" },
            text
        );
        row.push_str("\r\n");
        w.write_all(row.as_bytes())?;
    }
    w.write_all(b"\r\n")?;
    w.flush()
}

fn gen_ragged(rng: &mut Rng, path: &Path) -> io::Result<()> {
    const ROWS: u64 = 100_000;
    let mut w = create(path)?;
    let mut row = String::with_capacity(96);
    for i in 0..ROWS {
        row.clear();
        let cols = rng.range(1, 12);
        for c in 0..cols {
            if c > 0 {
                row.push(',');
            }
            if c == 0 {
                let _ = write!(row, "{}", i);
            } else if rng.below(3) == 0 {
                push_field(&mut row, rng.pick(NOTES_WORDS), ',');
            } else {
                let _ = write!(row, "{}", rng.range(0, 9999));
            }
        }
        row.push('\n');
        w.write_all(row.as_bytes())?;
    }
    w.flush()
}

fn gen_simple(rng: &mut Rng, path: &Path, delim: char) -> io::Result<()> {
    const ROWS: u64 = 1_000;
    let mut w = create(path)?;
    let mut row = String::with_capacity(96);
    for i in 0..ROWS {
        row.clear();
        for c in 0..8 {
            if c > 0 {
                row.push(delim);
            }
            match c {
                0 => {
                    let _ = write!(row, "{}", i);
                }
                1 => row.push_str(rng.pick(FIRST)),
                2 => row.push_str(rng.pick(LAST)),
                3 => row.push_str(rng.pick(CITY)),
                4 => row.push_str(&amount(rng)),
                5 => row.push_str(&date(rng)),
                6 => row.push_str(rng.pick(CATEGORY)),
                _ => {
                    let _ = write!(row, "{}", rng.below(2));
                }
            }
        }
        row.push('\n');
        w.write_all(row.as_bytes())?;
    }
    w.flush()
}

fn gen_backslash(rng: &mut Rng, path: &Path) -> io::Result<()> {
    const ROWS: u64 = 1_000;
    let mut w = create(path)?;
    let mut row = String::with_capacity(96);
    for i in 0..ROWS {
        row.clear();
        let _ = write!(row, "{}", i);
        row.push(',');
        // Delimiter escaped with the backslash escape char instead of quotes.
        row.push_str(if i % 2 == 0 { "a\\,b" } else { "plain" });
        row.push(',');
        row.push_str(if i % 3 == 0 {
            "say \\\"hi\\\""
        } else {
            "no quote"
        });
        row.push(',');
        row.push_str(rng.pick(NOTES_WORDS));
        // Trailing escape char: the state machine only consumes an escape that has a next char.
        row.push('\\');
        row.push('\n');
        w.write_all(row.as_bytes())?;
    }
    w.flush()
}

fn gen_utf8bom(rng: &mut Rng, path: &Path) -> io::Result<()> {
    const ROWS: u64 = 100_000;
    let mut w = create(path)?;
    w.write_all(b"\xEF\xBB\xBF")?;
    let mut row = String::with_capacity(64);
    for i in 0..ROWS {
        row.clear();
        let _ = writeln!(row, "{},{}", i, rng.pick(NOTES_WORDS));
        w.write_all(row.as_bytes())?;
    }
    w.flush()
}

fn latin1_bytes(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| {
            let v = c as u32;
            assert!(v <= 0xFF, "char {:?} is not Latin-1", c);
            v as u8
        })
        .collect()
}

fn gen_latin1(rng: &mut Rng, path: &Path) -> io::Result<()> {
    const ROWS: u64 = 1_000_000;
    const WORDS: &[&str] = &[
        "straße",
        "größe",
        "café",
        "über",
        "fuß",
        "naïve",
        "señor",
        "grüße",
        "péché",
        "garçon",
        "smörgås",
        "bäcker",
    ];
    let mut w = create(path)?;
    let mut row = String::with_capacity(64);
    let mut buf = Vec::with_capacity(96);
    for i in 0..ROWS {
        if i % 200_000 == 0 {
            eprintln!("  latin1.csv: {} rows", i);
        }
        row.clear();
        let _ = write!(
            row,
            "{};{};{};{}",
            i,
            WORDS[rng.below(WORDS.len() as u64) as usize],
            rng.range(0, 9999),
            if rng.below(2) == 0 { "ja" } else { "nein" }
        );
        row.push('\n');
        buf.clear();
        buf.extend_from_slice(&latin1_bytes(&row));
        w.write_all(&buf)?;
    }
    w.flush()
}

fn gen_win1252(rng: &mut Rng, path: &Path) -> io::Result<()> {
    const ROWS: u64 = 1_000_000;
    const WORDS: &[&str] = &[
        "€100",
        "“quoted”",
        "–dash–",
        "price €",
        "“no”",
        "naïve €",
        "rock – roll",
        "“fine”",
    ];
    let mut w = create(path)?;
    let mut row = String::with_capacity(64);
    for i in 0..ROWS {
        if i % 200_000 == 0 {
            eprintln!("  win1252.csv: {} rows", i);
        }
        row.clear();
        let _ = write!(
            row,
            "{};{};{};",
            i,
            WORDS[rng.below(WORDS.len() as u64) as usize],
            rng.range(0, 9999)
        );
        row.push_str(rng.pick(CITY));
        row.push('\n');
        let (bytes, _, _) = encoding_rs::WINDOWS_1252.encode(&row);
        w.write_all(&bytes)?;
    }
    w.flush()
}

/// Encodes `s` as UTF-16 without a BOM (the caller writes the BOM once per file).
fn utf16_bytes(s: &str, big_endian: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    for u in s.encode_utf16() {
        if big_endian {
            out.extend_from_slice(&u.to_be_bytes());
        } else {
            out.extend_from_slice(&u.to_le_bytes());
        }
    }
    out
}

fn gen_utf16(
    rng: &mut Rng,
    path: &Path,
    big_endian: bool,
    crlf: bool,
    delim: char,
    rows: u64,
) -> io::Result<()> {
    const TEXT: &[&str] = &[
        "plain",
        "日本語",
        "emoji 😀 here",
        "中文测试",
        "한국어",
        "🚀 rocket",
        "äöü",
        "mixed 文本",
    ];
    let mut w = create(path)?;
    w.write_all(&utf16_bytes("\u{FEFF}", big_endian))?;
    let mut row = String::with_capacity(64);
    let end = if crlf { "\r\n" } else { "\n" };
    for i in 0..rows {
        if i % 200_000 == 0 {
            eprintln!("  utf16: {} rows", i);
        }
        row.clear();
        let _ = write!(
            row,
            "{}{}{}{}{}{}{}",
            i,
            delim,
            TEXT[rng.below(TEXT.len() as u64) as usize],
            delim,
            rng.range(0, 99999),
            delim,
            rng.pick(CATEGORY)
        );
        row.push_str(end);
        let buf = utf16_bytes(&row, big_endian);
        w.write_all(&buf)?;
    }
    w.flush()
}

fn write_edge(dir: &Path) -> io::Result<u64> {
    let files: [(&str, &[u8]); 11] = [
        ("empty.csv", b""),
        ("header_only.csv", b"a,b,c\n"),
        ("single_cell.csv", b"x"),
        ("no_trailing_newline.csv", b"a,b\n1,2"),
        ("blank_lines.csv", b"a,b\n1,2\n\n3,4\n\n"),
        ("nul_bytes.csv", b"a,b\n1,\x00\x32\n\x00,3\n"),
        ("cr_only.csv", b"a,b\r1,2\r3,4\r"),
        ("unterminated_quote.csv", b"a,b\n1,\"open\n2,3\n"),
        ("invalid_utf8.csv", b"a,b\n1,na\xc3\x28ve\n\xff,2\n"),
        ("latin1_c1.csv", b"a,b\n1,x\x80y\n2,\x9f\x93\n"),
        ("ctrl_z.csv", b"a,b\n1,x\x1ay\n2,z\n"),
    ];
    let mut total = 0u64;
    for (name, bytes) in files {
        total += bytes.len() as u64;
        fs::write(dir.join(name), bytes)?;
    }
    Ok(total)
}

fn gen(
    path: &Path,
    seed: u64,
    f: impl FnOnce(&mut Rng, &Path) -> io::Result<()>,
) -> io::Result<u64> {
    let mut rng = Rng::new(seed);
    f(&mut rng, path)?;
    Ok(fs::metadata(path)?.len())
}

fn main() -> io::Result<()> {
    let root = PathBuf::from(env::args().nth(1).unwrap_or_else(|| "testdata".to_string()));
    fs::create_dir_all(&root)?;
    let edge = root.join("edge");
    fs::create_dir_all(&edge)?;

    let mut total = 0u64;
    let mut report = |name: &str, bytes: u64| {
        total += bytes;
        println!("{:<24} {:>12} bytes", name, bytes);
    };

    report("big.csv", gen(&root.join("big.csv"), 0xB16, gen_big)?);
    report("wide.csv", gen(&root.join("wide.csv"), 0x0A1, gen_wide)?);
    report(
        "quoted.csv",
        gen(&root.join("quoted.csv"), 0x007, gen_quoted)?,
    );
    report(
        "ragged.csv",
        gen(&root.join("ragged.csv"), 0x0A6, gen_ragged)?,
    );
    report(
        "semicolon.csv",
        gen(&root.join("semicolon.csv"), 0x5E1, |r, p| {
            gen_simple(r, p, ';')
        })?,
    );
    report(
        "tab.tsv",
        gen(&root.join("tab.tsv"), 0x7AB, |r, p| gen_simple(r, p, '\t'))?,
    );
    report(
        "pipe.csv",
        gen(&root.join("pipe.csv"), 0x91E, |r, p| gen_simple(r, p, '|'))?,
    );
    report(
        "backslash.csv",
        gen(&root.join("backslash.csv"), 0x0B5, gen_backslash)?,
    );
    report(
        "utf8bom.csv",
        gen(&root.join("utf8bom.csv"), 0x808, gen_utf8bom)?,
    );
    report(
        "latin1.csv",
        gen(&root.join("latin1.csv"), 0x1A7, gen_latin1)?,
    );
    report(
        "win1252.csv",
        gen(&root.join("win1252.csv"), 0xC25, gen_win1252)?,
    );
    report(
        "utf16le.csv",
        gen(&root.join("utf16le.csv"), 0x16E, |r, p| {
            gen_utf16(r, p, false, true, '\t', 1_000_000)
        })?,
    );
    report(
        "utf16be.csv",
        gen(&root.join("utf16be.csv"), 0x16B, |r, p| {
            gen_utf16(r, p, true, false, ',', 100_000)
        })?,
    );
    report("edge/*", write_edge(&edge)?);

    println!("total {} bytes", total);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::utf16_bytes;

    #[test]
    fn utf16_rows_carry_no_bom() {
        assert_eq!(utf16_bytes("a\n", false), [0x61, 0x00, 0x0A, 0x00]);
        assert_eq!(utf16_bytes("a\n", true), [0x00, 0x61, 0x00, 0x0A]);
    }
}

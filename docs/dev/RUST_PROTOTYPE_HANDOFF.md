# Handoff: Tablecruncher → Rust + GPUI prototype

**Audience:** the LLM agent that builds the prototype.
**Status:** plan approved, nothing implemented yet.
**Repo:** local fork of `https://github.com/forward-press/tablecruncher.git`, working branch `rewrite/rust/rust-main` (based on `main`).

---

## 0. How to work with this document

1. Read the whole document before writing code.
2. Decisions in §3 are **final**. Do not re-evaluate them. If one proves impossible, follow §12 (stop-and-ask).
3. Work phase by phase (§7–§11). Do not start a phase before the previous phase's exit criteria pass.
4. Record every measured number in `rust/BENCHMARKS.md` (template in §13). No claim about speed without a number.
5. Commit at the end of each phase on branch `rewrite/rust/rust-main`. One commit per phase minimum; message `proto: phase N – <summary>`.
6. Keep it a prototype: smallest code that meets the exit criteria. No speculative abstractions, no plugin systems, no config for values that never change.

---

## 1. Context

**Tablecruncher** is a CSV editor for huge files (README claim: 2 GB / 16 M rows opened in 32 s on a Mac Mini M2). C++17, UI toolkit FLTK 1.4, built with CMake. ~10.5 k lines of own code in `src/`, plus vendored libraries in `external/` (Duktape JS engine, cpp-httplib, nlohmann/json, utfcpp, utf8-cpp-utils).

### 1.1 The user's goals (in priority order)

1. Same speed as the C++ app, or better.
2. Better UI, built with GPUI.
3. Runs on macOS, Windows and Linux.
4. Cross-platform changes become easy (today: CMake + self-built FLTK per OS + vendored C/C++ libs = "quite the mess").
5. Easier maintenance.

The prototype exists to **prove or disprove** these goals with numbers and a working app. It is not the full port.

### 1.2 C++ source map (read these when a behaviour question comes up)

| File | Lines | What it is | Consult for |
|---|---|---|---|
| `src/csvparser.cpp` | 405 | CSV state machine, line reading, UTF-16 reading | §5.2 parser rules |
| `src/csvdatastorage.cpp` | 563 | Row storage (one `std::string` per row, fields joined by byte `0xFA`), sort | §5.6 sort |
| `src/csvtable.cpp` | 1245 | Controller: header row, find, replace, save, JSON export, flags | §5.5 save, §5.7 find |
| `src/csvapplication.cpp` | 3601 | App logic + all dialogs; `guessDefinition` (L870), `guessEncoding` (L997), `sort` dialog (L1855) | §5.1 detection |
| `src/csvwindow.cpp` | 916 | Per-document window; `loadFile` (L332) = the open flow | §5.1 |
| `src/csvgrid.cpp` | 613 | `Fl_Table` grid; `draw_cell` (L191) | §8 grid look |
| `src/macro.cpp` | 283 | Duktape macro API | §5.8 macros |
| `src/helper.cpp` | 798 | Encoding converters, number parsing, header guess (L514), column names (L60) | §5 |
| `src/colorthemes.cpp` | 199 | 4 colour themes | §8 themes |
| `src/globals.hh` | 240 | Constants, `CsvDefinition` | defaults |
| `docs/user/docs-1.8.ascii` | – | User docs incl. macro API (≈L280–340) | §5.8, §10 |

Known C++ performance problems the prototype must beat: `istream` line-by-line parsing (~65 MB/s), 16 M heap-allocated row strings, every cell read copies, sort copies two strings per comparison (and case-folds both for ignore-case), case-insensitive find case-folds the whole row per cell, regex find runs a JS program through the Duktape interpreter per cell, table-level undo copies the whole table, and reading a full row (save, copy, macros) is quadratic in the column count: `CsvDataStorage::row()` calls `get()` per column and `getColumn()` copies the whole row string and rescans it from the start (500-column `wide.csv`: 26 s to save 145 MB). The UI thread does all work and pumps events via ~40 `Fl::check()` calls.

---

## 2. Hard rules

- **Do not modify anything under `src/`, `external/`, `scripts/`, or the root `CMakeLists.txt`.** The C++ app is the baseline and must stay buildable.
- All Rust code lives in `rust/`. The C++ benchmark harness lives in `bench/cpp/`.
- Add to `.gitignore`: `/rust/target/`, `/bench/cpp/build/`, `/testdata/`.
- Never commit test data. It is generated (§7.2).
- Commit `rust/Cargo.lock`.
- Only the dependencies listed in §3.2. A new dependency requires asking the user (§12).
- No `unsafe` in our code.
- Do not use the Tablecruncher name or logo in the prototype UI (the README asks forks to rebrand). Working name: **TC Prototype**, binary `tc-proto`.
- Primary development and benchmark machine: the user's **Windows 11** PC. Linux: WSL2 or a VM for smoke tests only (not for perf numbers). macOS: CI build only, unless the user provides a Mac.

---

## 3. Final decisions

### 3.1 Architecture

A Cargo workspace in `rust/` with exactly two crates:

```
rust/
  Cargo.toml              # [workspace] members = ["core", "app"], shared [profile.release]
  rust-toolchain.toml     # pinned stable version (see §7.1)
  BENCHMARKS.md
  PROTOTYPE_REPORT.md     # written in phase 4
  core/                   # package "tc-core": no UI dependency, testable/benchmarkable headless
    src/lib.rs
    src/encoding.rs       # detect, decode to UTF-8, encode on save
    src/parse.rs          # row state machine, index pass, field decoding, dialect + header guess
    src/table.rs          # Base (immutable) + View state (mutable), cell access, edits
    src/ops.rs            # sort, find
    src/save.rs           # writer
    src/macros.rs         # rquickjs (phase 3)
    src/progress.rs       # Progress/cancel handle
    src/bin/tc-bench.rs
    src/bin/gen-testdata.rs
  app/                    # package "tc-app", binary "tc-proto": GPUI UI
    src/main.rs           # app init, actions, keybindings, menus, window opening
    src/doc_view.rs       # window root view: owns the document, status bar, dialog orchestration
    src/grid.rs           # the table grid
    src/dialogs.rs        # format, sort, find, jump-to-cell, macro window
    src/theme.rs          # Bright/Dark colours
bench/cpp/
  CMakeLists.txt          # standalone project, reuses src/*.cpp
  tc_bench_cpp.cpp
```

Merge files if they stay small. Do not split further.

`[profile.release]` in `rust/Cargo.toml`:

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
debug = "line-tables-only"   # keeps profiling usable
```

### 3.2 Dependencies (the complete allowed list)

| Crate | Where | Replaces / purpose |
|---|---|---|
| `gpui-kit` (exact pin `=0.7.x`, the latest 0.7 patch at setup) | app | FLTK. Re-exports GPUI (it pins `gpui-pre`) and gpui-component. Do **not** add `gpui` separately. |
| `anyhow` | app | error plumbing (GPUI APIs use it) |
| `encoding_rs` | core | Windows-1252 and UTF-16 decoding, Windows-1252 encoding |
| `memchr` | core | fast byte search (`memmem`) |
| `rayon` | core | parallel sort and find |
| `regex` | core | find (all modes), header-guess regexes |
| `rquickjs` | core (phase 3) | Duktape. QuickJS; compiles its C code in its build script. |
| `libc` (`cfg(unix)`) | core, bench only | peak RSS via `getrusage` |
| `windows-sys` (`cfg(windows)`, feature `Win32_System_ProcessStatus`, `Win32_System_Threading`, `Win32_Foundation`) | core, bench only | peak RSS via `GetProcessMemoryInfo` |

Dropped for the prototype: cpp-httplib (update check), nlohmann/json (JSON export is out of scope), utfcpp (Rust `str` is UTF-8), `Fl_Preferences` (no persisted settings).
Deliberately **not** used: `csv`/`csv-core` (the C++ quote semantics in §5.2 are non-standard; a direct port of the state machine is simpler and exact), `memmap2` (Windows can't overwrite a mapped file, which breaks Save to the same path).

### 3.3 Data model (this is where the speed comes from)

```rust
/// Immutable after load. Shared with background tasks as Arc<Base>. NEVER clone it.
pub struct Base {
    text: String,            // whole file, decoded to UTF-8, BOM removed
    row_starts: Vec<u64>,    // byte offset of each physical record; len = records + 1 (last = text.len())
    n_cols: u32,             // widest record (C++ pads every row to this)
    row_len_histogram: BTreeMap<u32, u64>, // field count -> number of records (for the "N different row lengths" warning)
    dialect: Dialect,
}

/// Small mutable state, owned by the document view. Cloned into background tasks when needed.
pub struct View {
    order: Vec<u32>,                 // display row -> record id. Sort/insert/delete only touch this.
    header: Option<u32>,             // record id used as header row, removed from `order` while active
    edits: HashMap<u32, Vec<String>>,// record id -> full replacement row (padded to n_cols)
    flags: HashSet<u32>,             // flagged record ids (follow rows through sort)
    dirty: bool,
}
```

Rules:
- Display row `i` → record id `order[i]`. All UI/macro coordinates are display rows; data rows exclude the header when the header is on (same as C++).
- Cell read: if the record is in `edits`, return from there; otherwise slice `text[row_starts[id]..row_starts[id+1]]`, run the field decoder (§5.2), return field `col` (empty string when the record is shorter). Return `Cow<str>`: borrowed when the field needs no unescaping, owned otherwise.
- Cells are **never** split at load time. Only visible rows (~50) are decoded per frame.
- Header on: `header = Some(order.remove(0))`. Header off: `order.insert(0, id)`. (Fixes the C++ design note in `csvdatastorage.hh:48`.)
- Record ids are `u32` (C++ limit is `int`, so no regression). Files with more than `u32::MAX` records → error message.
- Memory per row: 8 B (`row_starts`) + 4 B (`order`) vs. ≈50 B overhead per row in C++.

### 3.4 Threading

- Every long operation (read+decode, index, sort, find, save, macro) runs on the GPUI background executor and takes a `&Progress`:
  ```rust
  pub struct Progress { pub done: AtomicU64, pub total: AtomicU64, pub cancel: AtomicBool }
  ```
  Long ops check `cancel` at least every 64 MB or 100 k rows and return `Err(Cancelled)`.
- Background tasks get `Arc<Base>` plus a **clone** of the `View` parts they need (cloning `order` for 16 M rows = 64 MB, ~10 ms; acceptable). They return a result (e.g. a new `order`) which the UI thread applies.
- While an operation runs, the document is read-only (scrolling allowed, editing disabled). A status-bar progress indicator with a **Cancel** button is shown. Cancel must leave the document unchanged (save: remove the temp file, original untouched).

### 3.5 UI toolkit decisions

- `gpui-kit` components for: text input (cell editor, dialogs), buttons, checkboxes, dropdowns, modal dialogs, menus, notifications, theme (light/dark).
- **The grid is our own code, not gpui-kit's table.** Reason: GPUI positions are `f32` pixels. 16 M rows × ~24 px ≈ 384 M px; `f32` can only represent every 32nd pixel there, so any pixel-based virtual list jitters/skips rows near the bottom. Our grid scrolls by **row index** (§9.4), so content height never exceeds the viewport.
- Native file dialogs via GPUI (`prompt_for_paths` / `prompt_for_new_path`). macOS: native menu bar via GPUI `set_menus`. Windows/Linux: in-window menu bar.
- Shortcuts use GPUI's `secondary-` modifier (Cmd on macOS, Ctrl elsewhere). This replaces every `FL_COMMAND` / `#ifdef __APPLE__` branch.
- Exact GPUI/gpui-kit API names change between versions. **Always check the pinned version**: `cargo doc -p gpui-kit --open` and the gpui-kit repo examples at the matching tag. Do not guess API names from memory.

---

## 4. Scope

### In scope

Open (auto-detect + "Open with format" dialog), grid view with virtual scrolling for 16 M+ rows, selection, cell editing, header-row toggle (auto-guess at load), sort dialog, find (plain / case-insensitive / regex, find next), save / save as in the original encoding, copy selection, jump to cell, font size +/-/default, Bright + Dark theme, status bar, unsaved-changes prompt, multiple windows (one document per window), drag-and-drop open, macros (phase 3) incl. `flagRow` with flagged-row highlighting, benchmarks, parity tests, CI build on all three OSes.

### Out of scope (do not build)

Undo/redo UI, replace / replace all, paste, insert/delete/move rows or columns, split/merge columns, split CSV, JSON export, export flagged rows, flag menu commands, data consistency check, preferences, recent files, saved macro list, update check, Solarized themes, font chooser, onboarding, installers/signing/notarization (phase 4 stretch only), parallel indexing.

The data model must not block these later (e.g. column ops via a column-order `Vec<u32>`, undo via `order` snapshots), but implement none of them.

---

## 5. Behaviour spec (parity with C++)

Parity target: for the same input and the same dialect, **"load → save" output of the Rust core is byte-identical to the C++ harness output**, except the deliberate divergences in §5.9.

### 5.1 Open flow and detection

C++ reference: `CsvWindow::loadFile` (`csvwindow.cpp:332`), `guessDefinition` (`csvapplication.cpp:870`), `guessEncoding` (`csvapplication.cpp:997`), `Helper::guessHasHeader` (`helper.cpp:514`).

Rust open flow (all on background executor except dialogs):
1. Read the whole file into a `Vec<u8>` (pre-allocate file size; read in 64 MB chunks for progress/cancel).
2. **BOM**: `EF BB BF` → UTF-8, 3 bytes; `FF FE 00 00` → UTF-32LE; `00 00 FE FF` → UTF-32BE; `FE FF` → UTF-16BE, 2 bytes; `FF FE` → UTF-16LE, 2 bytes. UTF-32 → error "UTF-32 is not supported" (C++ doesn't support it either).
3. No BOM: `std::str::from_utf8` on the whole buffer (SIMD-fast, no size limit). Valid → UTF-8. Invalid → encoding **unknown**, preselect Windows-1252 in the format dialog.
4. Decode to a UTF-8 `String` (§5.3). Valid UTF-8 input must not be copied (`String::from_utf8(vec)`).
5. **Delimiter guess** on the decoded text, first 10 records, per candidate dialect. Candidates (quote always `"`):
   `,` esc `"` · `;` esc `"` · `\t` esc `"` · `|` esc `"` · `:` esc `"` · `,` esc `\` · `;` esc `\`
   For each candidate compute on the probed records (skip record 0 unless there is exactly one record):
   - `cols` = max field count; `shorter` = number of records with fewer fields than `cols`.
   - Penalty: if delimiter is `:` or `|` or escape is `\` → `cols = cols * 70 / 100` (integer).
   - If `cols <= 1 && shorter == 0` → `variance = 999`, else `variance = shorter`.
   Sort by `variance` ascending, then `cols` descending. Winner = first.
   Confidence: start `1.0`; if winner `variance > 0` → `/= 2`; if winner `cols ==` second's `cols` → `/= 2`; if winner delimiter is `,` or `\t` → `+= (1 - c) * 0.5`.
6. Show the **format dialog** if: encoding unknown, or confidence < 0.6, or the user chose "Open with format…". Dialog: delimiter (`,` `;` Tab `|` `:` `*`), quote (`"` `'`), escape (`"` `\`), encoding (UTF-8, Latin-1, Windows-1252, UTF-16LE, UTF-16BE), live preview of the first 20 records, buttons Open / Cancel. Changing encoding re-decodes the raw bytes.
7. Index pass (§5.2) with progress. If result has 0 records or 0 columns → error "Could not open file with the chosen/guessed definition".
8. **Header guess** (only if ≥ 2 records): record 0 is a header unless **any** of its fields: parses as a number by `parse_f64_prefix` (§5.4), fully matches the email regex `(\w+)(\.|_)?(\w*)@(\w+)(\.(\w+))+`, fully matches the date regex `\d+[.|\-]\d+[.|\-]\d+`, contains `://`, contains any of `{}*?!,.;"'§$%&/\=+`, or is empty. Use ASCII `\w`/`\d` (`(?-u)` or explicit classes) — C++ `std::regex` is ASCII.
9. If `row_len_histogram.len() != 1` → non-blocking warning: "Found N different row lengths. Please check that the definition is correct." The histogram counts every record's raw field count before padding (blank lines count as 1 field), as in C++.
10. Status bar: "Opened <name> in <x.y> s".

### 5.2 Parser (port exactly)

C++ reference: `CsvParser::parseCsvStream`, `parseCsvLine`, `myGetline` (`csvparser.cpp`).

**Physical lines** (after decoding to UTF-8): a line ends at `\n`, `\r\n`, or a lone `\r`. NUL bytes (`\0`) are dropped.

**Record state machine** — state carried across physical lines: `enclosed`, current field buffer, current field list. At the start of **each physical line**: `start_field = true`; `enclosed` keeps its value from the previous line. For each char `ch` at index `i` of the line:

1. If `escape != quote` and `ch == escape` and `i` is not the last char of the line → append line[i+1] to the field, skip it, continue.
2. If `ch == quote`:
   - if line[i+1] exists and `== quote`: if `enclosed` → append one quote, skip the next char; else → `enclosed = true` (do **not** skip the next char). Continue.
   - else: if `enclosed` → `enclosed = false`; else if `start_field` → `enclosed = true`; else → append the quote literally. Continue.
3. `start_field = false`.
4. If `ch == delimiter` and not `enclosed` → push the field, clear it, `start_field = true`, continue.
5. Append `ch`.

End of physical line: if not `enclosed` → push the field; the record is complete. If `enclosed` → the record continues on the next line and `"\n"` is appended to the field (so CRLF inside quotes becomes LF).

End of input: the final line is a record even without a line terminator; there is no extra empty record after the final terminator (C++ creates one and deletes it again). A blank line in the middle or at the end (i.e. `\n\n`) is a record with one empty field. **Unterminated quote at EOF:** close the field and emit the record (divergence D1).

Implementation:
- **Index pass** (load time, single-threaded): walk the text once with the state machine in "count only" mode: record start offsets into `row_starts`, count fields per record (→ `n_cols`, histogram). Use a 256-entry byte class table; bytes that are not delimiter/quote/escape/CR/LF/NUL are skipped cheaply. Target ≥ 1 GB/s on UTF-8 input. Only optimise further (e.g. `memchr`) if the phase-1 load target is missed.
- **Field decoder** (per record on demand): same state machine over `text[row_starts[id]..row_starts[id+1]]`, producing `Vec<Cow<str>>`. A field is `Borrowed` when it contains no quote, no escape, no CR, no NUL; otherwise build an owned `String`.
- Pad to `n_cols` with empty fields when returning a full row.
- Put both behind one function with a "count only" flag or share the state-machine step — do not maintain two diverging copies.

### 5.3 Encodings

| Encoding | Decode (to UTF-8) | Encode (on save) |
|---|---|---|
| UTF-8 | `String::from_utf8`; if the user forces UTF-8 on invalid bytes → `String::from_utf8_lossy` | identity, **no BOM** (C++ never writes a UTF-8 BOM – parity) |
| Latin-1 | each byte → `char` of the same value (by hand; `encoding_rs`'s "ISO-8859-1" label is actually Windows-1252) | char ≤ U+00FF → byte, else byte `0x7F` |
| Windows-1252 | `encoding_rs::WINDOWS_1252.decode_without_bom_handling` | `encoding_rs` encoder without replacement; unmappable → byte `0x7F` (C++ behaviour) |
| UTF-16LE/BE | `encoding_rs::UTF_16LE/BE.decode_without_bom_handling` after skipping the BOM | write BOM (`FF FE` / `FE FF`), then `str::encode_utf16()` → `to_le_bytes`/`to_be_bytes` (`encoding_rs` cannot encode UTF-16) |

### 5.4 Number parsing (shared helper)

`parse_f64_prefix(s) -> Option<f64>` mimics `std::stod`: skip leading ASCII whitespace; optional sign; then either `inf`/`infinity`/`nan` (case-insensitive) or `digits [. digits] [e|E [sign] digits]` with at least one digit in the mantissa; parse the **longest valid prefix** (`"12abc"` → 12, `"1e3x"` → 1000); nothing valid → `None`. Hex floats are not supported (rare; accepted divergence).

`parse_i64_prefix(s) -> Option<i64>` mimics `std::stol`: skip whitespace, optional sign, base-10 digits, longest prefix, overflow → `None`.

### 5.5 Save

C++ reference: `CsvTable::saveCsv`, `vec2string`, `toBeQuoted` (`csvtable.cpp:690`, `:1137`, `:1163`).

- Write to `<target>.tmp-<random>` in the same directory, `fsync`, then `std::fs::rename` over the target (replaces on all three OSes). On error or cancel remove the temp file.
- UTF-16: BOM first. If the header is on, the header record is written first.
- Each row: all `n_cols` fields (padding short rows with empty fields), joined by the delimiter, terminated by `\n` (C++ always uses `\n`).
- Field quoting (RFC style, the only style in the prototype): quote if the field contains `\n`, the quote char, or the delimiter. Quoting = quote char + field with every quote char doubled + quote char. (The escape char is never used on output.)
- **Fast path** (required for the save target): take an unedited record's raw slice **without its line terminator** (`\n`, `\r\n` or `\r`). If it contains no quote char, no escape char, no CR, no LF and no NUL, write it as is + `(n_cols − field_count)` delimiters + `\n`. This is byte-identical to re-encoding.
- Encode per §5.3 after building UTF-8 output; for UTF-8, write the fast path directly without re-encoding. Use a `BufWriter` with ≥ 1 MB buffer. Single-threaded first; parallelise encoding in chunks only if the save target is missed.
- Default extension when the user types a name without a dot: `.csv`.
- After a successful Save / Save As: clear `dirty`, update title and path.

### 5.6 Sort

C++ reference: `CsvDataStorage::sort` (`csvdatastorage.cpp:118`), dialog `CsvApplication::sort` (`csvapplication.cpp:1855`).

- Dialog: column (header names, or generic names A, B, … when the header is off; preselect the cursor's column), order (Ascending/Descending), type (Numeric / String / String ignore case). Default type = Numeric if the first 1,000,000 cells of the column consist only of ASCII digits (empty counts as digits – this is what the buggy C++ `isNumber` actually does), else String.
- Sorts data rows only (the header is not in `order`).
- Numeric key: `parse_f64_prefix(cell).unwrap_or(0.0)`, compared with `f64::total_cmp`.
- String key: byte-wise comparison of the UTF-8 cell (same as `std::string` `<`).
- Ignore case: compare lazily with `a.chars().flat_map(char::to_lowercase)` vs. the same for `b` — no per-row allocation.
- Build `Vec<(key, u32 record id)>` where string keys borrow from `Base.text` (`Cow<str>`), then `rayon` `par_sort_by` (stable). Descending = reversed comparator (not reversed result, so ties stay stable). Result = new `order`.
- Memory: measure peak RSS during sort. If it exceeds the C++ peak, switch string keys to `(offset: u64, len: u32)` into `Base.text` with owned fallback only for decoded/edited cells.

### 5.7 Find

C++ reference: `CsvTable::findSubstring` / `findInCell` / `nextField` (`csvtable.cpp:272–455`).

- Find bar: text, "Case sensitive" checkbox, "Regex" checkbox, Find Next button (`secondary-g` and `F3`), Enter = Find Next, Escape closes.
- Search area: the selection if more than one cell is selected, otherwise the whole table. Order: row-major starting at the cell **after** the current cell, wrapping around the area, ending at the current cell.
- One matcher for all modes: `regex::Regex` built from `regex::escape(text)` (plain) or the user pattern (regex mode), prefixed with `(?i)` when not case-sensitive. Invalid regex → inline error, no search.
- **Row prefilter** (plain and case-insensitive modes only, and only when the needle contains none of: quote char, escape char, CR, LF, NUL): run the matcher as `regex::bytes::Regex` on the raw record slice; skip the record if no match. Regex mode: no prefilter (anchors would give false negatives); decode every record and test each cell.
- Edited records are always checked through `edits`, never via the raw slice.
- Parallel: split the (wrapped) row range into chunks, `rayon` `find_first` over chunks; within a chunk scan in order. The first match in search order wins.
- Found → select + scroll to the cell, paint it with `cell_found_bg`. Not found → status "Not found".

### 5.8 Macros (phase 3)

C++ reference: `src/macro.cpp`, `docs/user/docs-1.8.ascii` (macro section ≈ L280–340).

Prepend exactly: `var _ROWS = <rows>; var _COLS = <cols>; ` then `var ROWMIN = r0; var COLMIN = c0; var ROWMAX = r1; var COLMAX = c1;\n` (selection, inclusive, display rows) then the user source.

| Function | Behaviour |
|---|---|
| `getString(r, c)` | Both args must be numbers, else returns `undefined`. Out of range → `""`. Returns the cell string. |
| `getInt(r, c)` | `parse_i64_prefix(cell)` → number; `None` → `undefined`. (C++ wraps to int32; we return the full value – D15.) |
| `getFloat(r, c)` | `parse_f64_prefix(cell)` → number; `None` → `undefined`. |
| `setCell(r, c, v)` | r, c numbers. `v` string → as is; number → `format!("{:.6}", v)` then strip trailing `0`s, then strip trailing `.`s (NaN → `"nan"`, ±inf → `"inf"`/`"-inf"`); anything else → `""`. Out of range → ignored. Marks the document modified. |
| `flagRow(r)` | Adds the record at display row `r` to `flags`. |
| `println(a, b, …)` | Formats each arg like `setCell` (strings as is, numbers formatted, others `""`), concatenates **without separator**, appends `" \n"`, sends to the macro log. |

Execution: fresh `rquickjs` Runtime + Context per run on a background thread. It works on a snapshot (Arc<Base> + cloned `View`); writes go to the snapshot's `edits`/`flags`; on success the snapshot's `edits`, `flags` and `dirty` replace the document's on the UI thread. Error or Cancel → discard all changes (D12). Cancel via `Runtime::set_interrupt_handler` polling `Progress.cancel`. Set a memory limit of 1 GB on the runtime.

### 5.9 Deliberate divergences from C++ (document these in the report; exclude them from parity)

| # | C++ behaviour | Rust behaviour |
|---|---|---|
| D1 | Unterminated quote at EOF: last record is lost **and** the previous real row is deleted | Record kept, field closed at EOF |
| D2 | Delimiter probe list bug (`;`+`\` probe overwritten by `*`, duplicate `,` probe) | Clean list in §5.1 |
| D3 | Delimiter guessed on raw bytes before encoding detection (breaks UTF-16) | Encoding first, then delimiter guess on decoded text |
| D4 | UTF-8 validity only checked for files < 200 MB; bigger files always get the format dialog | Whole file checked |
| D5 | Latin-1 bytes 0x80–0x9F dropped | Kept as U+0080–U+009F (lossless round trip) |
| D6 | Invalid UTF-8 replaced per utfcpp rules | `from_utf8_lossy` rules; U+FFFD count may differ |
| D7 | UTF-16: only LF ends a line | Lone CR also ends a line (same rule as 8-bit files) |
| D8 | Unstable `std::sort` | Stable sort; tie order may differ |
| D9 | Full Unicode case folding (ß = ss) | `to_lowercase` / regex `(?i)` simple folding |
| D10 | Regex find uses JavaScript regex syntax | Rust `regex` syntax (no look-around, no back-references) |
| D11 | Flags are row positions (don't follow sort) | Flags follow records |
| D12 | Macro error keeps partial changes | Error/cancel discards the run's changes |
| D13 | JS globals persist between macro runs (one Duktape heap) | Fresh context per run |
| D14 | Sort shortcut Cmd+Ctrl+S = Ctrl+S on Windows/Linux (collides with Save) | Sort = `secondary-alt-s` |
| D15 | `getInt` wraps to int32 | Full i64 value |
| D16 | On Windows the file is opened in text mode (`Helper::openInputStream` defaults to `std::ios_base::in`): a `0x1A` byte ends the file and everything after it is silently lost (verified with the harness) | Files are always read as binary |

C++ quirks **kept** for parity: no UTF-8 BOM written on save; `\n` line endings on save; short rows padded to `n_cols`; blank lines become rows; NUL bytes removed; CRLF inside quoted fields becomes LF; numeric sort treats unparsable cells as 0.

---

## 6. Global performance targets

Measured on the user's Windows PC, release builds, same files, median of 3 runs after one warm-up run (§7.5).

| Metric (file) | Hard limit | Aim |
|---|---|---|
| Load: read + decode + index (`big.csv`) | ≤ 1.0 × C++ | ≤ 0.5 × C++ |
| Peak RSS after load (`big.csv`) | ≤ 1.0 × C++ | ≤ 0.6 × C++ |
| Save (`big.csv`) | ≤ 1.0 × C++ | ≤ 0.5 × C++ |
| Sort numeric / string / ignore-case (`big.csv`) | ≤ 1.0 × C++ each | ≤ 0.3 × C++ |
| Peak RSS during sort (`big.csv`) | ≤ 1.0 × C++ | – |
| Find plain + case-insensitive, needle in last row (`big.csv`) | ≤ 1.0 × C++ | ≤ 0.2 × C++ |
| Find regex, needle in last row (`quoted.csv`) | ≤ 1.0 × C++ | – |
| Macro: scale column over 1 M rows (`quoted.csv`) | ≤ 1.0 × C++ | – |
| GUI: UI thread blocked during any long op | never > 100 ms | – |
| GUI: grid frame time while scrolling (50 rows × 20 cols visible) | p99 < 16 ms | median < 8 ms |
| GUI: Open → first rows visible (`big.csv`) | ≤ C++ load time | – |
| GUI: RAM with `big.csv` loaded, idle | ≤ C++ | – |

---

## 7. Phase 0 — Setup and C++ baseline

### 7.1 Toolchains

- [ ] All OSes: install Rust via rustup, `rustup component add rustfmt clippy`. Create `rust/rust-toolchain.toml` with `channel = "<current stable, e.g. 1.xx.y>"` (the version from `rustc --version`) and `components = ["rustfmt", "clippy"]`. It must satisfy gpui-kit's MSRV.
- [ ] **Windows** (user's machine): Visual Studio 2022 with "Desktop development with C++" is installed. Use the **"x64 Native Tools Command Prompt for VS 2022"** for C++ builds (the plain Developer prompt targets x86). FLTK 1.4.5 is at `C:\dev\fltk-1.4.5`, built in `C:\dev\fltk-1.4.5\build-nmake` (`FLTKINCDIR=C:\dev\fltk-1.4.5`, `FLTKLIBDIR=C:\dev\fltk-1.4.5\build-nmake`). Existing app build output: `build\dist\Tablecruncher.exe`.
- [ ] **macOS** (CI, or a Mac if provided): full **Xcode** (GPUI compiles Metal shaders at build time; Command Line Tools alone are not enough). `sudo xcode-select --switch /Applications/Xcode.app` and verify `xcrun -find metal`.
- [ ] **Linux** (Debian/Ubuntu, WSL2 or VM): `sudo apt install build-essential pkg-config libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libxcb1-dev libvulkan-dev mesa-vulkan-drivers`. If linking fails, check the dependency list in Zed's `script/linux` and add what is missing. Without a GPU, Mesa's lavapipe (software Vulkan) is used — this doubles as the "no GPU" test.
- [ ] Work on branch `rewrite/rust/rust-main` (based on `main`); add the `.gitignore` entries from §2.

### 7.2 Test data generator (`tc-core/src/bin/gen-testdata.rs`)

`cargo run --release -p tc-core --bin gen-testdata -- ../testdata` (path relative to `rust/`). Deterministic: inline splitmix64 PRNG with a fixed seed (no `rand` crate). Generate:

| File | Content | Dialect |
|---|---|---|
| `big.csv` | Header + 16,000,000 rows, 10 columns, **2.0 GB ± 10 %**, LF. Columns: `id` = `(i * 2654435761) mod 2^32` (unique, scrambled), `first_name`, `last_name` (ASCII + some non-ASCII like `Zoë`, `José`), `email`, `city` (incl. `São Paulo`, `München`), `amount` (e.g. `-1234.56`), `date` (`YYYY-MM-DD`), `category`, `notes` (20–60 chars; every 20th row contains a comma → quoted, every 50th a doubled quote, every 1000th an embedded newline), `flag` (0/1). Last row's `notes` contains `NEEDLE_7f3a`. | `,` `"` `"` UTF-8 |
| `wide.csv` | Header + 100,000 rows × 500 short columns | `,` UTF-8 |
| `quoted.csv` | Header + 1,000,000 rows, **CRLF**, heavy quoting: embedded CRLF, doubled quotes, delimiters inside quotes, `""` empty fields, quote mid-field (`ab"c`), text after closing quote (`"ab"c`), a trailing blank line. Column 2 numeric (for the macro bench). Last row contains `NEEDLE_7f3a`. | `,` UTF-8 |
| `ragged.csv` | 100,000 rows with 1–12 fields | `,` UTF-8 |
| `semicolon.csv`, `tab.tsv`, `pipe.csv` | 1,000 rows each | `;` / Tab / `\|` |
| `backslash.csv` | 1,000 rows using `\"` and `\,` escapes | `,` quote `"` escape `\` |
| `utf8bom.csv` | 100,000 rows with UTF-8 BOM | `,` UTF-8, BOM 3 |
| `latin1.csv` | 1,000,000 rows, `äöüßé…` — **no bytes 0x80–0x9F** | `;` Latin-1 |
| `win1252.csv` | 1,000,000 rows incl. `€` `“` `”` `–` | `;` Windows-1252 |
| `utf16le.csv` | 1,000,000 rows, BOM, CRLF, incl. emoji (surrogate pairs) and CJK | Tab, UTF-16LE, BOM 2 |
| `utf16be.csv` | 100,000 rows, BOM, LF | `,` UTF-16BE, BOM 2 |
| `edge/*.csv` | `empty` (0 bytes), `header_only`, `single_cell` (`x`, no newline), `no_trailing_newline`, `blank_lines` (middle + end), `nul_bytes`, `cr_only` (lone CR endings), `unterminated_quote`, `invalid_utf8`, `latin1_c1` (Latin-1 with 0x80–0x9F), `ctrl_z` (byte 0x1A mid-file) | `,` UTF-8 (`latin1_c1`: Latin-1) |

Expected parity diffs: `edge/unterminated_quote` (D1), `edge/invalid_utf8` (D6), `edge/latin1_c1` (D5), `edge/ctrl_z` (D16, Windows only). Everything else must be byte-identical.

### 7.3 C++ benchmark harness (`bench/cpp/`)

Standalone CMake project; does **not** touch the root `CMakeLists.txt`.

- Sources: `../../src/csvparser.cpp`, `csvdatastorage.cpp`, `csvtable.cpp`, `helper.cpp`, `macro.cpp`, `../../external/duktape/duktape.c`, and `tc_bench_cpp.cpp`.
- Include dirs: `../../src`, `../../external`, FLTK include dir. Compile definition `TCRUNCHER_PARSER_WITHOUT_UPDATE` (removes the parser's UI dependency). Same flags as the root project (C++17, Release `-O2`; MSVC: `/utf-8 /DWIN32_LEAN_AND_MEAN /DNOMINMAX /D_FILE_OFFSET_BITS=64`). Link FLTK exactly like the root `CMakeLists.txt` (FLTK is needed for `Fl::check()` in sort), plus `psapi` on Windows and `-framework CoreFoundation -framework Cocoa` on macOS.
- `tc_bench_cpp.cpp` defines the globals the sources expect: `Macro macro;` and `void updateMacroLogBuffer(void*, std::string) {}`.
- Behaviour:
  1. Parse CLI (§7.4). Build a `CsvDefinition` from it (`delimiter`, `quote`, `escape`, `encoding`, `bomBytes`).
  2. **load**: open with `Helper::openInputStream(input, path)` (same call as the app), `CsvParser().parseCsvStream(&input, table.getStorage(), &def)`, `table.updateInternals()`, `table.setDefinition(def)`. Construct the table as `CsvTable table(0, 0)`: the default constructor (`csvtable.cpp:37`) builds a temporary and leaves `headerRow` uninitialised.
  3. **save**: `table.saveCsv(out + ".saved.csv", noop_cb, nullptr)`.
  4. **find_cs / find_ci / find_re** (if given): `table.findSubstring(needle, 0, 0, {0, 0, rows-1, cols-1}, caseSensitive, useRegex)`.
  5. **macro** (if given): `macro.execute(&table, {r0, c0, r1, c1}, source)`.
  6. **sort** (if given): `table.sortTable(col, !desc, type)` then **save_sorted** to `out + ".sorted.csv"`.
  7. After each step print one JSON line (§7.4). Peak RSS: Windows `GetProcessMemoryInfo(...).PeakWorkingSetSize`; macOS `getrusage` `ru_maxrss` (bytes); Linux `ru_maxrss` (KiB).
- If `Fl::check()` misbehaves without a display on Linux, run under a desktop session or `xvfb-run`.

### 7.4 Shared benchmark CLI and output (C++ harness and `tc-bench` must match)

```
<bin> --file F --delim <char|tab> --quote <char> --escape <char>
      --enc utf8|latin1|win1252|utf16le|utf16be --bom <n> --out <prefix>
      [--find TEXT] [--find-ci TEXT] [--find-re PATTERN]
      [--macro FILE.js --macro-sel r0,c0,r1,c1]
      [--sort-col N --sort-type num|str|stri [--sort-desc]]
```

Output: one JSON object per step on stdout:
`{"tool":"cpp","file":"big.csv","step":"load","ms":31234,"rows":16000001,"cols":10,"peak_rss_mb":4321}`
Steps in order: `load`, `save`, `find_cs`, `find_ci`, `find_re`, `macro`, `sort`, `save_sorted` (skip steps without flags). For find, add `"found":[r,c]`.

`tc-bench` uses **only the public `tc-core` API** (the same code paths as the app) and **explicit** dialects (no auto-detection), so both tools parse identically. The harness does no header switching; neither does `tc-bench`.

### 7.5 Benchmark protocol

- Release builds only. Close heavy apps, plugged in, same disk.
- One warm-up run (fills the OS file cache), then 3 measured runs; report the median.
- `big.csv` runs: load+save; `--find NEEDLE_7f3a --find-ci needle_7F3A`; three sort runs: `--sort-col 5 --sort-type num`, `--sort-col 2 --sort-type str`, `--sort-col 4 --sort-type stri`; parity sort run `--sort-col 0 --sort-type num` (unique keys → byte-identical sorted output).
- `quoted.csv` runs: `--find-re "NEEDLE_[0-9a-f]{4}"`; macro run (phase 3).
- Record everything in `rust/BENCHMARKS.md` (§13).

### Phase 0 exit criteria

- [ ] C++ harness builds and runs on Windows; baseline numbers for every row of §6 that applies to the core are in `BENCHMARKS.md`.
- [ ] C++ `.saved.csv` outputs exist for every test file (the parity oracle).
- [ ] Commit.

---

## 8. Phase 1 — `tc-core` headless

### Steps

1. [ ] `progress.rs`: `Progress`, `Cancelled`.
2. [ ] `encoding.rs`: BOM detection, UTF-8 check, decode (§5.3), encode (§5.3). Unit tests: round trip for every encoding, incl. surrogate pairs and `€`.
3. [ ] `parse.rs`: shared state machine (§5.2), index pass, field decoder, `parse_f64_prefix`, `parse_i64_prefix`, dialect guess (§5.1.5), header guess (§5.1.8). Unit tests for every rule in §5.2 (one small string per rule), for D1, and for detection on each generated file (expected dialect = the table in §7.2; for `latin1.csv` and `win1252.csv` the expected result is "encoding unknown" + delimiter `;`).
4. [ ] `table.rs`: `Base`, `View`, `Document::open(raw bytes, dialect, &Progress)`, `rows()`, `cols()`, `cell()`, `row()`, `header_name(col)` (header text, or generic name `A…Z, AA…` when off/empty — port `Helper::createGenericColumnNames`), `set_cell()`, `set_header(bool)`.
5. [ ] `save.rs` (§5.5) incl. fast path.
6. [ ] `ops.rs`: sort (§5.6), find (§5.7).
7. [ ] `bin/tc-bench.rs` (§7.4) with peak-RSS helper.
8. [ ] Parity run: for every file in §7.2, compare `tc-bench` `.saved.csv` with the C++ one byte for byte (`fc /b` on Windows, `cmp` elsewhere). Also compare `.sorted.csv` for the `--sort-col 0` run. All must match except the expected diffs.
9. [ ] Benchmarks per §7.5; fill the Rust column of `BENCHMARKS.md`.
10. [ ] If a hard limit is missed: profile with `samply` (works on all three OSes), fix the top hotspot, re-measure. **One** optimisation round per missed metric, then stop-and-ask (§12) if still missed.

### Phase 1 exit criteria

- [ ] `cargo test -p tc-core` passes; `cargo clippy -- -D warnings` clean; `cargo fmt --check` clean.
- [ ] Parity: all byte-identical except the listed expected diffs.
- [ ] Every core hard limit in §6 met.
- [ ] Commit.

---

## 9. Phase 2 — GPUI app (`tc-app`)

### 9.1 Skeleton
1. [ ] Add `gpui-kit` with an exact pin (`=0.7.x`). Follow its README for init (`gpui_kit::application().run(...)`, `gpui_kit::init(cx)`, `gpui_kit::open_window(...)` at the time of writing — verify against the pinned version).
2. [ ] Window title `TC Prototype`, or `<file name>` + ` *` when dirty.

### 9.2 Actions, shortcuts, menus
Define GPUI actions and bind keys (`secondary` = Cmd/Ctrl):

| Menu | Item | Key |
|---|---|---|
| File | Open… | `secondary-o` |
| File | Open with format… | `secondary-shift-o` |
| File | Close | `secondary-w` |
| File | Save | `secondary-s` |
| File | Save As… | `secondary-shift-s` |
| File | Quit (macOS: app menu) | `secondary-q` |
| Edit | Copy | `secondary-c` |
| Edit | Edit cell | `enter`, `secondary-enter` |
| Edit | Select all | `secondary-a` |
| Data | Find… | `secondary-f` |
| Data | Find next | `secondary-g`, `f3` |
| Data | Sort… | `secondary-alt-s` |
| Macro | Execute macro… | `secondary-e` |
| View | Toggle header row | `secondary-shift-h` |
| View | Jump to cell… | `secondary-l` |
| View | Bigger / Smaller / Default font | `secondary-=` / `secondary--` / `secondary-0` |
| View | Theme: Bright / Dark | – |

macOS: `cx.set_menus(...)` (native). Windows/Linux: use gpui-kit's in-window app menu bar component if the pinned version has one; otherwise a row of buttons each opening a gpui-kit dropdown menu dispatching the same actions. **One action list drives both.**

### 9.3 Open
1. [ ] Native picker filtered to `csv`, `tsv`, `txt`. Drag-and-drop of a file onto a window opens it too.
2. [ ] If the current window already has a document, open a new window.
3. [ ] Background: read → detect → (format dialog on UI thread if needed, §5.1.6) → decode → index → header guess. Progress + Cancel in the status bar.
4. [ ] Column widths: from the first 10,000 rows + header, `width = clamp((min(max_chars, 2 * avg_chars) + 2) * 0.6 * font_size, 60, 400)` px.

### 9.4 Grid (`grid.rs`) — the critical piece
State: `top_row: u64`, `y_offset: Pixels` in `[0, row_height)`, `scroll_x: Pixels`, `col_widths: Vec<Pixels>`, selection `anchor: (u64, u32)`, `cursor: (u64, u32)`, `editing: Option<(u64, u32)>`.

1. [ ] **Vertical scrolling by index**: wheel/trackpad delta is added to `y_offset`; whole rows carry into `top_row`; clamp to `[0, rows − visible_rows]`. Never create an element as tall as the whole table.
2. [ ] **Custom vertical scrollbar**: thumb position = `top_row / max_top` (compute in `f64`); dragging maps back with `f64`; minimum thumb height 24 px. Horizontal scrolling can use normal pixel offsets (500 cols × 400 px fits `f32` easily) plus a normal scrollbar.
3. [ ] **Rendering**: `visible_rows = ceil(viewport_h / row_h) + 1`. Per frame, decode each visible record once (`Document::row`). Only visible columns are rendered. Start with plain `div`s absolutely positioned inside a clipped container; move to a custom `Element` **only** if the frame-time target is missed.
4. [ ] Sticky column header row (header names or A, B, …; bold; centred) and sticky row-number column (display row + 1, grouped every 3 digits with a hair space U+200A, as in C++; width from the digit count of `rows`).
5. [ ] Cell text: first line only, clipped, at most 1,000 chars passed to the text system. Draw the two-dot hint (bottom-right, `cell_ellipsis` colour) if the cell has a newline or `chars × 0.6 × font_size > column width`.
6. [ ] Colours by state: selected / flagged / selected+flagged / found cell (see `draw_cell` in `csvgrid.cpp:191`).
7. [ ] Row height = font size + 10 px. Font size default 14, range 8–30.
8. [ ] Mouse: click selects; shift-click extends; drag selects (auto-scroll when dragging past an edge); double-click edits; column header click selects the column; row header click selects the row; drag a column header's right border to resize (min 30 px).
9. [ ] Keyboard: arrows move, shift+arrows extend, PageUp/PageDown, `secondary-up`/`secondary-down` first/last row, `secondary-left`/`secondary-right` first/last column, Tab / shift-Tab next/previous cell. The cursor always stays visible (scroll into view).
10. [ ] Instrumentation: in debug and release, when an env var `TC_TRACE=1` is set, log the grid's frame time (render → paint) to stderr and log any UI-thread task longer than 100 ms.

### 9.5 Editing, clipboard, header
1. [ ] Edit: gpui-kit input overlaid on the cell rect; Enter/blur commits, Escape cancels. Commit → `set_cell` → `dirty = true`. Editing is disabled while a long op runs.
2. [ ] Copy: selection as CSV lines with the document's delimiter, RFC quoting (§5.5), `\r\n` after every line (C++ `encodeCsvLine`), to the system clipboard.
3. [ ] Toggle header row: `View::set_header`, redraw; the status bar selection uses header names when on.

### 9.6 Dialogs
1. [ ] Format dialog (§5.1.6) with a 20-row preview grid (a simple non-virtual table is fine here).
2. [ ] Sort dialog (§5.6) → background sort → apply `order`.
3. [ ] Find bar (§5.7) → background find → select + scroll + highlight.
4. [ ] Jump to cell: input accepts `123` (row) or `B123` (column + row) → select + scroll.
5. [ ] Unsaved changes on close/quit: GPUI prompt "Save changes to <name>?" Save / Don't Save / Cancel.
6. [ ] Errors (open/save failures) as gpui-kit notifications or prompts, never panics.

### 9.7 Status bar and themes
1. [ ] Status bar: selection (`A1` or `A1 > C5`, with `header:row` when the header is on), `rows × cols`, dialect (`UTF-8 · , · "`), last operation message/time, progress + Cancel while running.
2. [ ] Themes: port the **Bright** and **Dark** entries of `src/colorthemes.cpp` (`cell_bg`, `cell_text`, `selection_bg`, `selection_text`, `selection_flagged_bg`, `selection_flagged_text`, `cell_flagged_bg`, `cell_found_bg`, `cell_ellipsis`, `header_row_bg`, `header_row_text`, `grid_border`, `win_bg`, `win_text`) into a `struct Colors` in `theme.rs`; switch the gpui-kit theme mode (light/dark) together with it.

### 9.8 Cross-platform run
1. [ ] Windows: full manual test (the Phase 2 exit criteria below) with `big.csv`.
2. [ ] Linux (WSL2 or VM, lavapipe): open `quoted.csv`, scroll, edit, sort, find, save. Record whether it runs and how it feels (no perf numbers).
3. [ ] macOS: CI build (phase 4). Manual run only if a Mac is available.

### Phase 2 exit criteria (Windows, `big.csv`)
- [ ] `secondary-down` shows the last row with the correct `id`; scrolling up and down one notch at a time near the bottom shows consecutive row numbers with no jumps or jitter.
- [ ] All GUI rows of §6 met (log with `TC_TRACE=1`, note values in `BENCHMARKS.md`).
- [ ] Open → edit a cell in row 15,999,999 → sort → find → save → reopen: the edit and the sort order survive.
- [ ] Cancel works for open, sort, find, save and leaves the document/file unchanged.
- [ ] Linux smoke test result recorded.
- [ ] Commit.

---

## 10. Phase 3 — Macros

1. [ ] `macros.rs` per §5.8 with `rquickjs`.
2. [ ] Macro window: multi-line code input (gpui-kit input in multi-line/code mode), Run (`secondary-enter`), Cancel (while running), log area. Keep the last source in memory for the session. Selection = current grid selection.
3. [ ] Tests in `tc-core`: run every example from the macro section of `docs/user/docs-1.8.ascii` on a small table and assert the resulting cells; test every row of the §5.8 table (incl. `setCell` number formatting `0.5 → "0.5"`, `100 → "100"`, `1e-7 → "0"`, and `println("a", 1)` → `"a1 \n"`).
4. [ ] Add `--macro`/`--macro-sel` to `tc-bench`. Benchmark on `quoted.csv`: a macro that multiplies column 2 by 1.1 for all rows, vs. the C++ harness.

### Phase 3 exit criteria
- [ ] Doc examples produce the same results as in C++ (run them in the C++ app or harness to confirm).
- [ ] Macro hard limit in §6 met; a macro with `while(true){}` can be cancelled within 1 s.
- [ ] Commit.

---

## 11. Phase 4 — CI, report, decision

1. [ ] If the fork is on GitHub: `.github/workflows/rust.yml`, matrix `windows-latest`, `macos-latest`, `ubuntu-latest`: `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test -p tc-core`, `cargo build --release -p tc-app`. Linux job installs the apt packages from §7.1. If the fork is local-only, run the same commands on every available OS and note which were verified.
2. [ ] Stretch (only if everything else is done): `cargo-packager` producing unsigned `.app`/`.dmg`, `.msi` or NSIS `.exe`, AppImage. Note what signing/notarisation would still need.
3. [ ] Write `rust/PROTOTYPE_REPORT.md`:
   - One section per user goal (§1.1): verdict (met / partly / not met) + evidence (numbers, screenshots paths, CI links).
   - Build experience: exact steps per OS today (C++) vs. prototype, count of platform-specific code lines (`cfg!`/`#[cfg]`) in the prototype.
   - Divergences D1–D16 and any new ones.
   - GPUI experience: API friction, missing components, bugs hit, version pinning notes.
   - Remaining work for a full port (the out-of-scope list in §4, with rough size), top risks.
   - Recommendation: go / no-go.
4. [ ] Commit.

---

## 12. Stop-and-ask conditions

Stop, report what you found, and ask the user when:
- A §6 hard limit is still missed after one profiling/optimisation round.
- The pinned `gpui-kit` does not build or run on one of the three OSes.
- Index-based scrolling cannot be made jitter-free.
- You need a dependency not in §3.2.
- You need to change anything under `src/`, `external/`, `scripts/` or the root `CMakeLists.txt`.
- A parity diff appears that is not covered by §5.9.
- A decision in §3 turns out to be impossible.

---

## 13. `rust/BENCHMARKS.md` template

```markdown
# Benchmarks

Machine: <CPU, RAM, disk, OS build>. Rust <version>, C++ compiler <version>, FLTK 1.4.5.
Each value: median of 3 runs after 1 warm-up. Times in ms, memory in MB.

| File | Step | C++ ms | Rust ms | Ratio | C++ peak MB | Rust peak MB | Hard limit met |
|---|---|---|---|---|---|---|---|
| big.csv | load | | | | | | |
| big.csv | save | | | | | | |
| big.csv | find_cs | | | | | | |
| big.csv | find_ci | | | | | | |
| big.csv | sort num (col 5) | | | | | | |
| big.csv | sort str (col 2) | | | | | | |
| big.csv | sort stri (col 4) | | | | | | |
| quoted.csv | find_re | | | | | | |
| quoted.csv | macro | | | | | | |

## GUI (Windows, big.csv, TC_TRACE=1)

| Metric | C++ | Rust | Limit met |
|---|---|---|---|
| Open → first rows visible | | | |
| Idle RAM with file loaded | | | |
| Grid frame time median / p99 | n/a | | |
| Longest UI-thread block during long ops | | | |

## Parity

| File | save identical | sorted identical | Note |
|---|---|---|---|
```

---

## 14. Quick reference: C++ constants worth reusing

- Default font size 14, min 8, max 30; row height = font size + 10 (`TCRUNCHER_ROW_HEIGHT_ADD`).
- Default column width 150 px; preview rows 20; auto-arrange probes 10,000 rows.
- Generic column names: `A`…`Z`, `AA`… (`Helper::createGenericColumnNames`).
- Status updates in C++: parser every 25,000 rows, save every 20,000 rows.

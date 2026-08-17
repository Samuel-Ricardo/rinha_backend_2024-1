# Storage Engine — `espora-db`

The standout component of this repository: a **hand-written embedded database** for the Rinha,
used in place of an external PostgreSQL/Redis — exactly the kind of maximally-local,
append-friendly store that fits an account-sharded credit/debit log under a 1.5 CPU / 550 MB cap.

Crate root: `rinha/espora-db/`. Modules (`src/lib.rs`):

```rust
pub mod error;
pub mod lock;
pub mod model;
mod test;

#[cfg(feature = "tokio")]
pub mod tokio;   // src/tokio.rs is EMPTY — see "Feature gate" warning
```

> The `model::tokio` async submodule is declared **unconditionally** by `model/mod.rs`, so the
> async engine is always compiled regardless of the (broken) `tokio` feature — see §6.

---

## 1. Conceptual model

- **Page-based**: fixed `PAGE_SIZE = 4096`-byte pages on disk.
- **Append-only rows**: rows are never updated or deleted; new rows fill the current page, and a
  new page is appended when the current one is full.
- **Fixed-size slots**: every row occupies exactly `ROW_SIZE` bytes within a page, so reads can
  seek directly to slot `i` by `(page_index, row_index)`.
- **`bitcode` serialization**: row payloads are encoded with `bitcode` (binary, compact) — not
  JSON — so any `Serialize + DeserializeOwned` Rust type can be a row.
- **Sync + async twins**: the same `Page`/`Builder`/`Error` are shared between a synchronous
  (`std::fs`) `Db` and an asynchronous (`tokio::fs`) `Db`.

## 2. The `Page` (`src/model/page.rs`)

```rust
pub struct Page<const ROW_SIZE: usize> {
    data: Vec<u8>,
    free: usize,
}

pub const PAGE_SIZE: usize = 4096;
```

A page is a 4096-byte buffer with a `free` byte counter. Available rows per page:

```rust
pub fn available_rows(&self) -> usize { (PAGE_SIZE - self.data.len()) / ROW_SIZE }
```

So `ROW_SIZE = 2048` → 2 rows/page; `ROW_SIZE = 1024` → 4 rows/page (the test asserts `4`).

### Slot layout

Each row slot is exactly `ROW_SIZE` bytes:

```
| 8-byte BE length | bitcode payload | zero-padding to ROW_SIZE |
```

### `Page::insert` (verbatim)

```rust
pub fn insert<S: Serialize>(&mut self, row: S) -> DbResult<()> {
    let serialized = bitcode::serialize(&row)?;
    let size = (serialized.len() as u64).to_be_bytes();

    let mut cursor = Cursor::new(&mut self.data);
    cursor.seek(SeekFrom::Start((PAGE_SIZE - self.free) as u64))?;

    self.free -= cursor.write(&size)?;
    self.free -= cursor.write(&serialized)?;
    self.free -= cursor.write(&vec![0; ROW_SIZE - (serialized.len() + size.len())])?;
    Ok(())
}
```

- The `free` pointer tracks remaining space; `seek` to `PAGE_SIZE - free` (the next free slot).
- Write: length prefix → payload → padding to `ROW_SIZE`.
- **No size check.** If the serialized payload exceeds `ROW_SIZE - 8`, the padding length
  computation underflows → latent panic/overflow. Callers must size `ROW_SIZE` generously.

### `Page::from_bytes` and the zero-prefix end-marker

Reconstruction scans slots; the first slot whose 8-byte prefix is all zeros marks end-of-data:

```rust
let last_page_offset = iter::from_fn(|| {
    let offset = cursor * ROW_SIZE;
    if offset + ROW_SIZE > data.len() { return None; }
    cursor += 1;
    if data[offset..offset + 8] != [0; 8] { Some(offset + ROW_SIZE) } else { None }
}).last().unwrap_or(0);
self.free = PAGE_SIZE - last_page_offset;
```

`rows()` yields each slot's raw bytes `[8..8+size]` until it hits a zero length prefix — there is
**no delete, no update, no row-id index**; rows are append-only and addressed by position.

## 3. Synchronous `Db` (`src/model/database.rs`)

```rust
pub struct Db<T, const ROM_SIZE: usize> {   // note: "ROM_SIZE" == ROW_SIZE, misnamed
    current_page: Page<ROM_SIZE>,
    reader: File,
    writer: File,
    last_sync: Instant,
    pub(crate) sync_writes: Option<Duration>,
    data: PhantomData<T>,
}
```

> ⚠ The const-generic is named **`ROM_SIZE`** in the sync struct/builder but **`ROW_SIZE`** in
> the impl block and the `Page`/async code. It is the **row slot size**, not read-only memory.

### Open (`from_path`) — resume the last page

```rust
let mut file = OpenOptions::new().read(true).write(true).create(true).open(&path)?;
let current_page = if file.seek(SeekFrom::End(-(PAGE_SIZE as i64))).is_ok() {
    let mut buf = vec![0; PAGE_SIZE];
    file.read_exact(&mut buf)?;
    file.seek(SeekFrom::End(-(PAGE_SIZE as i64)))?;
    Page::from_bytes(buf)
} else {
    file.seek(SeekFrom::End(0))?;
    Page::new()
};
// a *separate* read handle:
reader: File::open(&path)?,
writer: file,
```

- Opens RW+create; seeks to `End(-(PAGE_SIZE))` and loads the trailing page as `current_page`;
  repositions the writer at that page start so subsequent inserts **rewrite it in place**.
- A **separate** read handle (`File::open`) is kept for read paths.

### Insert / write path

```rust
pub fn insert(&mut self, row: T) -> DbResult<()> {
    self.current_page.insert(row)?;
    self.writer.write_all(
        &[ self.current_page.as_ref(), &vec![0; PAGE_SIZE - self.current_page.as_ref().len()] ].concat(),
    )?;
    match self.sync_writes {
        Some(interval) if self.last_sync.elapsed() > interval => {
            self.writer.sync_data()?;
            self.last_sync = Instant::now();
        }
        _ => {}
    }
    if self.current_page.available_rows() == 0 {
        self.current_page = Page::new();
    } else {
        self.writer.seek(io::SeekFrom::End(-(PAGE_SIZE as i64)))?;
    }
    Ok(())
}
```

### Storage-engine characterization

- **Per-insert write amplification**: the entire 4096-byte page is rewritten (via
  `[page.as_ref(), pad].concat()` — an extra `Vec` allocation + memcpy per insert).
- **Append vs rewrite**: the **current/last page is rewritten in full** on every insert;
  a **new** page is appended only when the current one fills (then `seek` is skipped → writes
  continue past it at EOF).
- **Durability default** = `Some(Duration::from_secs(0))` → `elapsed() > 0ns` is always true →
  **`fsync` on every insert**. This is correct-for-safety but a major RPS cap; tuned via the builder.
- **No mmap** — plain `Read`/`Seek`/`Write` on `std::fs::File`.
- **Clobber bug**: if the file already ends in a *completely full* page, `from_path` loads it as
  `current_page` (with `available_rows()==0`); the next insert rewrites (destroys) existing
  data instead of appending a new page. The "full page" branch is only handled **after** writing.

### Read path

- `pages()` / `pages_reverse()` sequentially scan pages (`seek(Start cursor*PAGE_SIZE)` /
  `seek(End(-cursor*PAGE_SIZE))`), `read_exact` per page.
- `rows()` / `rows_reverse()` flat-map pages into `bitcode::deserialize::<T>` results.
  `rows_reverse()` reverses rows **within each page**; pages are emitted newest-first, so global
  ordering is newest-page-newest-row first — fine for "last 10 transactions".

## 4. Builder (`src/model/builder.rs`)

```rust
pub struct Builder { pub(crate) sync_writes: Option<Duration> }
impl Default for Builder { /* sync_writes: Some(Duration::from_secs(0)) */ }

pub fn sync_writes(self, sync_writes: bool) -> Self     // Some(0) or None
pub fn sync_write_interval(self, interval: Duration) -> Self
pub fn build<T, const ROM_SIZE: usize>(self, path) -> io::Result<Db<T, ROM_SIZE>>
```

**Durability knobs (the tuning surface for the Rinha):**

- `Builder::default()` → fsync every insert (safest, slowest).
- `.sync_writes(false)` → never fsync (fastest, loses data on crash).
- `.sync_write_interval(Duration::from_millis(…))` → fsync at most once per interval (good middle).

A commented-out `build_tokio` stub sits here:
`/* TODO: TOKIO ASYNC DB ... */` — the real async builder lives at `model/tokio/builder.rs`.

## 5. Asynchronous `Db` (`src/model/tokio/database.rs`)

```rust
pub struct Db<T, const ROW_SIZE: usize> {
    current_page: Page<ROW_SIZE>,
    reader: File,            // tokio::fs::File
    writer: File,
    last_sync: Instant,
    pub(crate) sync_writes: Option<Duration>,
    data: PhantomData<T>,
}
```

Mirrors the sync engine with `.await` on `seek`/`read_exact`/`write_all`/`sync_data`, reusing the
**sync** `Page`, `PAGE_SIZE`, `DbResult`, and `Builder`. The streaming read API uses the `futures`
`stream!` macro:

```rust
fn pages(&mut self) -> impl Stream<Item = Page<ROW_SIZE>> + '_ {
    let mut cursor = 0;
    stream! {
        loop {
            let offset = (cursor * PAGE_SIZE) as u64;
            if self.reader.seek(io::SeekFrom::Start(offset)).await.is_err() { break; }
            let mut buf = vec![0; PAGE_SIZE];
            cursor += 1;
            match self.reader.read_exact(&mut buf).await {
                Ok(n) if n > 0 => yield Page::<ROW_SIZE>::from_bytes(buf),
                _ => break,
            }
        }
    }
}
```

Public async API: `builder()`, `from_path()`, `insert()`, `lock_writes()`, and `rows()` /
`rows_reverse()` returning `impl Stream<Item = DbResult<T>>`. Async builder:
`model/tokio/builder.rs::build_tokio<T, const ROW_SIZE: usize>(...) -> io::Result<Db<T, ROW_SIZE>>`.

> ⚠ The async model **likely does not compile as written** (mixed Windows-only
> `std::os::windows::io` imports alongside Unix-only `self.writer.as_raw_fd()` +
> `libc::flock`; a `seek(SeekFrom::End(-offset))` on a `u64 offset` which is a unary-minus on an
> unsigned type — `E0600`). The sync engine is Windows-only. See [FINDINGS.md](FINDINGS.md).

## 6. File locking (`lock.rs`, `linux_lock.rs`) — unfinished

**`lock.rs` (Windows)** holds `LockHandle { fd: RawHandle }`; `Drop` calls `ReleaseMutex`.
**`linux_lock.rs` (Unix)** holds `LockHandle { fd: RawFd }`; `Drop` calls `libc::flock(LOCK_UN)`.

Sync `lock_writes` (Windows):

```rust
pub fn lock_writes(&mut self) -> DbResult<LockHandle> {
    let fd = self.writer.as_raw_handle() as *mut winapi::ctypes::c_void;
    match unsafe { LockFile(fd, 0, 0, 1, 0) } {
        0 => Ok(LockHandle { fd: … }),
        _ => Err(io::Error::new(io::ErrorKind::Other, "Could not lock file").into()),
    }
}
```

Async `lock_writes` uses `tokio::task::spawn_blocking` + a `oneshot` to run a blocking
`libc::flock(LOCK_EX)` off the executor — the right *pattern for blocking FFI in async* — but then
stores a **Windows** `LockHandle` (Drop → `ReleaseMutex`): acquire (flock) and release
(ReleaseMutex) are mismatched APIs. A `//TODO: REFACT ALL TO LIBC` confirms the rewrite is pending.

**Defects:**

1. `linux_lock.rs` is never declared as a module → orphaned dead code; only the Windows path is taken.
2. Win32 acquire uses `LockFile` (byte-range lock), but Drop releases via `ReleaseMutex` (kernel
   mutex API, wrong for a file range) → the scoped lock is effectively a no-op/incorrect.
3. `LockFile` returns nonzero on **success** (`0` = failure), but the code maps `0 => Ok(...)` →
   **inverted success/failure**.
4. The async path stores the Windows `LockHandle` while acquiring via Unix `flock`.

**Internal concurrency**: none beyond `&mut self`. `insert`/`pages`/`rows` require exclusive
access; the caller must wrap the `Db` in a `Mutex`. `lock_writes` is a **manual, external**,
inter-process advisory lock, **not** used internally by `insert`, and (per the defects above) not
actually functional.

## 7. Error handling (`src/error.rs`)

```rust
pub enum Error {
    Io(io::Error),
    Serialization(Box<dyn error::Error + Send + Sync>),
}
```

`From<io::Error>` and `From<bitcode::Error>` are provided. There is no corruption/page-format or
lock-acquisition variant; `lock_writes` failures are mapped to `Error::Io(...Other)`, and a
malformed `from_bytes` (truncated slots) yields a zero-`free` page silently rather than a typed error.

## 8. Serialization (`bitcode`)

- `Page::insert` calls `bitcode::serialize(&row)`; readers call `bitcode::deserialize(...)`.
- `bitcode` is pulled with the `serde` feature, so any `Serialize + DeserializeOwned` type works.
- **On-disk page→bytes**: `Page: AsRef<[u8]>` returns `&self.data`; `Db::insert` writes
  `page.as_ref()` then zero-pads up to `PAGE_SIZE`. Each page is exactly `PAGE_SIZE` bytes, fully
  self-describing via per-slot length prefixes — **no header, no page-id, no schema, no migration**.

## 9. Tests (`src/test/`)

`test/mod.rs` declares `mod page; mod tokio;` (both always compiled).

**`test/page.rs`** — 5 unit tests on `Page`:

- `initialize` / `initialize_with_empty_bytes` — invariants on `Page::<1024>::new()` and `from_bytes(vec![])`.
- `initialize_from_bytes` — insert two rows, reload, assert `len`/`available_rows`/`available_space`.
- `insert_into_page` — insert `String("Rinha")`, `String("De")`, `u64(2024)`; assert
  `available_rows()` 4→3→2→1 and round-trip via `bitcode::deserialize`.
- `update_existing_page` — reload an existing page and append two more rows; assert all four read in order.

**`test/tokio/database.rs`** — 2 async tests on the `Db`:

```rust
#[tokio::test]
async fn test_db_rows() {
    let mut db = Db::<i64, 2048>::from_path(tmp.join("test.espora")).await.unwrap();
    db.insert(1).await...;             // 1..5
    let row = db.rows().try_collect::<Vec<_>>().await.unwrap();
    assert_eq!(row, vec![1, 2, 3, 4, 5]);
}

#[tokio::test]
async fn test_db_rows_reverse() {
    … insert 1..=5 …
    let rows = db.rows_reverse().try_collect::<Vec<_>>().await.unwrap();
    assert_eq!(rows, vec![5, 4, 3, 2, 1]);
}
```

> ⚠ Both tests open the **same file** `temp_dir().join("test.espora")` with no cleanup; because
> `from_path` resumes the last page, a second run appends onto stale state, so `row.len()` would
> exceed 5 and the assertions would fail. Test isolation is missing.

## 10. Summary

| Aspect | Design | State |
|---|---|---|
| Storage | page-based, append-only, fixed slots, bitcode | implemented (sync) |
| Sync engine | `std::fs`, resume last page, rewrite-in-place | Windows-only, compiles |
| Async engine | `tokio::fs`, `stream!` reads | non-compiling as written |
| Durability | fsync-every-insert default, tunable | implemented + tested knobs |
| Cross-process lock | advisory LockFile / flock | broken (wrong release API, inverted result, orphaned Linux impl) |
| Error model | `Io` + `Serialization` only | no corruption/lock variant |
| Tests | Page unit (5) + async Db (2) | pass once, not isolated |

The engine is a solid, focused design for the Rinha's append-only log per account; the gaps are
the async compile errors, the broken cross-platform locking, and the clobber-on-full-page bug.

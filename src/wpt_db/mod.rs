//! SQLite storage for multi-engine WPT results.
//!
//! Raw wptreport.json files (from wpt.fyi or Blitz's own runner) are
//! stream-ingested into a normalized schema: test/subtest/area names are
//! interned (append-only, deduplicated via UNIQUE indexes), per-run results
//! are stored per test and per subtest, and area rollups are precomputed
//! with cross-engine union denominators.

use std::collections::HashMap;
use std::fmt;
use std::io::{BufReader, Read};
use std::sync::Mutex;

use rusqlite::{params, Connection, Transaction};
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;

/// Version of the on-disk format. Stored in SQLite's `user_version` pragma;
/// a database with a different version is deleted and rebuilt from scratch
/// (all data is re-ingestable from upstream sources).
const DB_VERSION: i64 = 3;

const SCHEMA: &str = include_str!("schema.sql");

/// The directory data files (SQLite databases) are stored in: the
/// `WPT_DATA_DIR` env var if set, otherwise a `.data` directory in the
/// repo root when run via cargo, falling back to `.data` in the current
/// working directory.
fn data_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("WPT_DATA_DIR") {
        return dir.into();
    }
    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    std::path::Path::new(&root).join(".data")
}

pub static WPT_COMPARE_DB: WptDb = WptDb::new();

pub struct WptDb(Mutex<Option<Connection>>);

impl WptDb {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// Run `f` with the (lazily opened) database connection.
    /// Should be called from a blocking context (`spawn_blocking`).
    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> T) -> T {
        let mut guard = self.0.lock().unwrap();
        let conn = guard.get_or_insert_with(|| {
            let dir = data_dir();
            std::fs::create_dir_all(&dir).expect("failed to create WPT data directory");
            let path = dir.join("wpt-compare.db");
            let conn = open_versioned(&path)
                .unwrap_or_else(|err| panic!("failed to open wpt-compare database: {err}"));
            conn.execute_batch(SCHEMA).unwrap();
            conn
        });
        f(conn)
    }
}

/// Open the database at `path`, deleting and recreating it if its stored
/// format version (SQLite `user_version`) doesn't match [`DB_VERSION`].
fn open_versioned(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let has_tables: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table')",
        [],
        |row| row.get(0),
    )?;

    if has_tables && version != DB_VERSION {
        println!(
            "wpt-compare database has format version {version}, expected {DB_VERSION}: rebuilding"
        );
        drop(conn);
        for suffix in ["", "-wal", "-shm"] {
            let mut sidecar = path.as_os_str().to_owned();
            sidecar.push(suffix);
            match std::fs::remove_file(std::path::Path::new(&sidecar)) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => panic!("failed to remove outdated wpt-compare database: {err}"),
            }
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "user_version", DB_VERSION)?;
        return Ok(conn);
    }

    if version != DB_VERSION {
        conn.pragma_update(None, "user_version", DB_VERSION)?;
    }
    Ok(conn)
}

pub fn status_str(status: i64) -> &'static str {
    match status {
        0 => "PASS",
        1 => "FAIL",
        2 => "ERROR",
        3 => "TIMEOUT",
        4 => "NOTRUN",
        5 => "CRASH",
        6 => "PRECONDITION_FAILED",
        7 => "SKIP",
        _ => "UNKNOWN",
    }
}

fn status_int(s: &str) -> i64 {
    match s {
        "PASS" | "OK" => 0,
        "FAIL" => 1,
        "ERROR" => 2,
        "TIMEOUT" => 3,
        "NOTRUN" => 4,
        "CRASH" => 5,
        "PRECONDITION_FAILED" => 6,
        "SKIP" => 7,
        _ => 8,
    }
}

/// Metadata for a run being ingested, taken from the wpt.fyi runs API
/// (or synthesized for Blitz's own report).
#[derive(Clone, Debug)]
pub struct RunMeta {
    pub product: String,
    pub browser_version: String,
    pub os: Option<String>,
    pub wpt_revision: String,
    pub run_time: Option<String>,
    pub source_run_id: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunRow {
    pub id: i64,
    pub product: String,
    pub browser_version: String,
    pub wpt_revision: String,
    pub run_time: Option<String>,
}

#[derive(Deserialize)]
struct TestResult {
    test: String,
    status: String,
    #[serde(default)]
    subtests: Vec<Subtest>,
    #[serde(default)]
    duration: Option<i64>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct Subtest {
    name: String,
    status: String,
    #[serde(default)]
    message: Option<String>,
}

struct IngestCtx<'tx> {
    tx: &'tx Transaction<'tx>,
    run_id: i64,
    store_messages: bool,
    /// Area name -> id. Small (~650 entries for /css/); test and subtest
    /// name interning is delegated to SQLite's UNIQUE indexes.
    areas: HashMap<String, i64>,
}

impl IngestCtx<'_> {
    fn load_areas(&mut self) {
        let mut stmt = self.tx.prepare("SELECT name, id FROM areas").unwrap();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            self.areas.insert(row.get(0).unwrap(), row.get(1).unwrap());
        }
    }

    fn intern_area(&mut self, name: &str) -> i64 {
        if let Some(&id) = self.areas.get(name) {
            return id;
        }
        let parent_id = name.rsplit_once('/').map(|(p, _)| self.intern_area(p));
        self.tx
            .execute(
                "INSERT INTO areas (name, parent_id) VALUES (?1, ?2)",
                params![name, parent_id],
            )
            .unwrap();
        let id = self.tx.last_insert_rowid();
        self.areas.insert(name.to_string(), id);
        id
    }

    fn ingest_test(&mut self, mut test: TestResult) {
        // Blitz's published report uses test names without a leading slash;
        // wpt.fyi reports include it. Normalize to the wpt.fyi convention.
        if !test.test.starts_with('/') {
            test.test.insert(0, '/');
        }
        // The /encoding/ suite is excluded (very large, not layout-relevant)
        if test.test.starts_with("/encoding/") {
            return;
        }

        let existing: Option<i64> = self
            .tx
            .prepare_cached("SELECT id FROM tests WHERE name = ?1")
            .unwrap()
            .query_row(params![test.test], |row| row.get(0))
            .ok();
        let test_id = match existing {
            Some(id) => id,
            None => {
                let area_name = test
                    .test
                    .rsplit_once('/')
                    .map(|(dir, _)| dir.trim_matches('/'))
                    .unwrap_or("")
                    .to_string();
                let area_id = self.intern_area(&area_name);
                self.tx
                    .prepare_cached("INSERT INTO tests (name, area_id) VALUES (?1, ?2)")
                    .unwrap()
                    .execute(params![test.test, area_id])
                    .unwrap();
                self.tx.last_insert_rowid()
            }
        };

        let (pass, total) = if test.subtests.is_empty() {
            ((test.status == "PASS" || test.status == "OK") as u32, 1u32)
        } else {
            let pass = test.subtests.iter().filter(|s| s.status == "PASS").count() as u32;
            (pass, test.subtests.len() as u32)
        };

        self.tx
            .prepare_cached(
                "INSERT OR REPLACE INTO results (run_id, test_id, status, subtest_pass, subtest_total, duration_ms, message)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .unwrap()
            .execute(params![
                self.run_id,
                test_id,
                status_int(&test.status),
                pass,
                total,
                test.duration,
                if self.store_messages { test.message.as_deref() } else { None },
            ])
            .unwrap();

        for subtest in test.subtests {
            let inserted: Option<i64> = self
                .tx
                .prepare_cached(
                    "INSERT INTO subtests (test_id, name) VALUES (?1, ?2)
                     ON CONFLICT(test_id, name) DO NOTHING
                     RETURNING id",
                )
                .unwrap()
                .query_row(params![test_id, subtest.name], |row| row.get(0))
                .ok();
            let subtest_id: i64 = match inserted {
                Some(id) => id,
                None => self
                    .tx
                    .prepare_cached("SELECT id FROM subtests WHERE test_id = ?1 AND name = ?2")
                    .unwrap()
                    .query_row(params![test_id, subtest.name], |row| row.get(0))
                    .unwrap(),
            };
            self.tx
                .prepare_cached(
                    "INSERT OR REPLACE INTO subtest_results (run_id, subtest_id, status, message)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .unwrap()
                .execute(params![
                    self.run_id,
                    subtest_id,
                    status_int(&subtest.status),
                    if self.store_messages {
                        subtest.message.as_deref()
                    } else {
                        None
                    },
                ])
                .unwrap();
        }
    }
}

struct ResultsSeed<'a, 'tx>(&'a mut IngestCtx<'tx>);
impl<'de> DeserializeSeed<'de> for ResultsSeed<'_, '_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for ResultsSeed<'_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a sequence of test results")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while let Some(test) = seq.next_element::<TestResult>()? {
            self.0.ingest_test(test);
        }
        Ok(())
    }
}

struct ReportSeed<'a, 'tx>(&'a mut IngestCtx<'tx>);
impl<'de> DeserializeSeed<'de> for ReportSeed<'_, '_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for ReportSeed<'_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a wptreport object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "results" => map.next_value_seed(ResultsSeed(self.0))?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}

/// True if a run matching this metadata has already been ingested.
pub fn run_exists(conn: &Connection, meta: &RunMeta) -> bool {
    if let Some(source_run_id) = meta.source_run_id {
        conn.query_row(
            "SELECT 1 FROM runs WHERE product = ?1 AND source_run_id = ?2",
            params![meta.product, source_run_id],
            |_| Ok(()),
        )
        .is_ok()
    } else {
        conn.query_row(
            "SELECT 1 FROM runs WHERE product = ?1 AND browser_version = ?2 AND wpt_revision = ?3",
            params![meta.product, meta.browser_version, meta.wpt_revision],
            |_| Ok(()),
        )
        .is_ok()
    }
}

/// Stream-ingest a wptreport.json from `reader` as a new run.
/// The report is deserialized incrementally so it is never buffered in full.
pub fn ingest_report(
    conn: &mut Connection,
    meta: &RunMeta,
    reader: impl Read,
) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO runs (product, browser_version, os, wpt_revision, run_time, source_run_id, ingested_at, is_latest)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'), 1)",
        params![
            meta.product,
            meta.browser_version,
            meta.os,
            meta.wpt_revision,
            meta.run_time,
            meta.source_run_id
        ],
    )?;
    let run_id = tx.last_insert_rowid();
    tx.execute(
        "UPDATE runs SET is_latest = 0 WHERE product = ?1 AND id != ?2",
        params![meta.product, run_id],
    )?;

    let mut ctx = IngestCtx {
        tx: &tx,
        run_id,
        store_messages: false,
        areas: HashMap::new(),
    };
    ctx.load_areas();

    let mut de = serde_json::Deserializer::from_reader(BufReader::with_capacity(1 << 20, reader));
    ReportSeed(&mut ctx).deserialize(&mut de)?;

    tx.commit()?;
    Ok(run_id)
}

/// Recompute `area_scores` for the latest run of each product, using
/// cross-engine union denominators: for each test the subtest denominator is
/// the max subtest total across the latest runs, and every test known to any
/// engine counts against every engine's totals (missing = 0 passes).
pub fn recompute_area_scores(conn: &mut Connection) {
    #[derive(Default, Clone, Copy)]
    struct Scores {
        tests_pass: u32,
        tests_total: u32,
        subtests_pass: u32,
        subtests_total: u32,
        interop_score_sum: u64,
    }

    let tx = conn.transaction().unwrap();

    let run_ids: Vec<i64> = {
        let mut stmt = tx
            .prepare("SELECT id FROM runs WHERE is_latest = 1 ORDER BY id")
            .unwrap();
        let ids = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        ids
    };

    // area_id -> parent area_id
    let parents: HashMap<i64, Option<i64>> = {
        let mut stmt = tx.prepare("SELECT id, parent_id FROM areas").unwrap();
        let map = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        map
    };

    // Per-test union denominator and area, across the latest runs.
    // (test_id, area_id, denom)
    let tests: Vec<(i64, i64, u32)> = {
        let mut stmt = tx
            .prepare(
                "SELECT t.id, t.area_id, MAX(r.subtest_total)
                 FROM tests t JOIN results r ON r.test_id = t.id
                 JOIN runs ON runs.id = r.run_id AND runs.is_latest = 1
                 GROUP BY t.id",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        rows
    };
    let mut scores: HashMap<(i64, i64), Scores> = HashMap::new();
    for &run_id in &run_ids {
        // Seed every known test into this run's rollup (missing = 0 passes)
        let mut per_test: HashMap<i64, u32> = HashMap::new();
        {
            let mut stmt = tx
                .prepare("SELECT test_id, subtest_pass FROM results WHERE run_id = ?1")
                .unwrap();
            let mut rows = stmt.query(params![run_id]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                per_test.insert(row.get(0).unwrap(), row.get(1).unwrap());
            }
        }
        for &(test_id, area_id, denom) in &tests {
            let pass = per_test.get(&test_id).copied().unwrap_or(0).min(denom);
            let denom = denom.max(1);
            let all_pass = (pass == denom) as u32;
            let interop = (pass as u64 * 1000) / denom as u64;
            let mut area = Some(area_id);
            while let Some(id) = area {
                let s = scores.entry((run_id, id)).or_default();
                s.tests_pass += all_pass;
                s.tests_total += 1;
                s.subtests_pass += pass;
                s.subtests_total += denom;
                s.interop_score_sum += interop;
                area = parents.get(&id).copied().flatten();
            }
        }
    }

    tx.execute("DELETE FROM area_scores", []).unwrap();
    {
        let mut insert = tx
            .prepare("INSERT INTO area_scores VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")
            .unwrap();
        for ((run_id, area_id), s) in &scores {
            insert
                .execute(params![
                    run_id,
                    area_id,
                    s.tests_pass,
                    s.tests_total,
                    s.subtests_pass,
                    s.subtests_total,
                    s.interop_score_sum as i64,
                ])
                .unwrap();
        }
    }

    tx.commit().unwrap();
}

/// Delete all non-latest runs and their per-run data, keeping only the
/// latest run of each product. Interned names (`areas`/`tests`/`subtests`)
/// are append-only and left in place. Freed pages stay on SQLite's freelist
/// for reuse by subsequent ingests, so the file size plateaus rather than
/// growing with history.
pub fn prune_old_runs(conn: &mut Connection) {
    const OLD_RUNS: &str = "SELECT id FROM runs WHERE is_latest = 0";
    let tx = conn.transaction().unwrap();
    for table in ["subtest_results", "results", "area_scores"] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE run_id IN ({OLD_RUNS})"),
            [],
        )
        .unwrap();
    }
    let pruned = tx
        .execute(&format!("DELETE FROM runs WHERE id IN ({OLD_RUNS})"), [])
        .unwrap();
    tx.commit().unwrap();
    if pruned > 0 {
        println!("Pruned {pruned} old WPT run(s)");
        // Keep the WAL file bounded after the bulk delete
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
    }
}

/// The latest run for each product, in ingestion order.
pub fn latest_runs(conn: &Connection) -> Vec<RunRow> {
    let mut stmt = conn
        .prepare(
            "SELECT id, product, browser_version, wpt_revision, run_time
             FROM runs WHERE is_latest = 1 ORDER BY id",
        )
        .unwrap();
    stmt.query_map([], |row| {
        Ok(RunRow {
            id: row.get(0)?,
            product: row.get(1)?,
            browser_version: row.get(2)?,
            wpt_revision: row.get(3)?,
            run_time: row.get(4)?,
        })
    })
    .unwrap()
    .map(|r| r.unwrap())
    .collect()
}

#[derive(Clone, Default, Copy, PartialEq)]
pub struct AreaScore {
    pub tests_pass: u32,
    pub tests_total: u32,
    pub subtests_pass: u32,
    pub subtests_total: u32,
    pub interop_score_sum: u64,
}

impl AreaScore {
    pub fn subtest_fraction(&self) -> f32 {
        if self.subtests_total == 0 {
            0.0
        } else {
            self.subtests_pass as f32 / self.subtests_total as f32
        }
    }

    pub fn interop_fraction(&self) -> f32 {
        if self.tests_total == 0 {
            0.0
        } else {
            (self.interop_score_sum as f64 / (self.tests_total as f64 * 1000.0)) as f32
        }
    }
}

/// Whether an area with this name exists. The empty string is the root
/// (the parent of all top-level areas).
pub fn area_exists(conn: &Connection, area: &str) -> bool {
    area.is_empty()
        || conn
            .query_row("SELECT 1 FROM areas WHERE name = ?1", params![area], |_| {
                Ok(())
            })
            .is_ok()
}

/// Scores for `area` itself, one entry per run (None if the run has no data).
pub fn area_score(conn: &Connection, run_ids: &[i64], area: &str) -> Vec<Option<AreaScore>> {
    // The root area has no row of its own: aggregate the top-level areas
    let sql = if area.is_empty() {
        "SELECT s.run_id, SUM(s.tests_pass), SUM(s.tests_total), SUM(s.subtests_pass), SUM(s.subtests_total), SUM(s.interop_score_sum)
         FROM area_scores s JOIN areas a ON a.id = s.area_id WHERE a.parent_id IS NULL AND ?1 = ''
         GROUP BY s.run_id"
    } else {
        "SELECT s.run_id, s.tests_pass, s.tests_total, s.subtests_pass, s.subtests_total, s.interop_score_sum
         FROM area_scores s JOIN areas a ON a.id = s.area_id WHERE a.name = ?1"
    };
    let mut stmt = conn.prepare_cached(sql).unwrap();
    let mut by_run: HashMap<i64, AreaScore> = HashMap::new();
    let mut rows = stmt.query(params![area]).unwrap();
    while let Some(row) = rows.next().unwrap() {
        by_run.insert(
            row.get(0).unwrap(),
            AreaScore {
                tests_pass: row.get(1).unwrap(),
                tests_total: row.get(2).unwrap(),
                subtests_pass: row.get(3).unwrap(),
                subtests_total: row.get(4).unwrap(),
                interop_score_sum: row.get::<_, i64>(5).unwrap() as u64,
            },
        );
    }
    run_ids.iter().map(|id| by_run.get(id).copied()).collect()
}

/// Sort order for area listings
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AreaSort {
    Alpha,
    /// By subtest count, largest first
    Subtests,
}

/// Direct child areas of `area` with per-run scores
pub fn child_area_scores(
    conn: &Connection,
    run_ids: &[i64],
    area: &str,
    sort: AreaSort,
) -> Vec<(String, Vec<Option<AreaScore>>)> {
    let sql = if area.is_empty() {
        "SELECT a.name, s.run_id, s.tests_pass, s.tests_total, s.subtests_pass, s.subtests_total, s.interop_score_sum
         FROM area_scores s JOIN areas a ON a.id = s.area_id
         WHERE a.parent_id IS NULL AND ?1 = ''"
    } else {
        "SELECT a.name, s.run_id, s.tests_pass, s.tests_total, s.subtests_pass, s.subtests_total, s.interop_score_sum
         FROM area_scores s JOIN areas a ON a.id = s.area_id
         WHERE a.parent_id = (SELECT id FROM areas WHERE name = ?1)"
    };
    let mut stmt = conn.prepare_cached(sql).unwrap();
    let mut by_area: HashMap<String, HashMap<i64, AreaScore>> = HashMap::new();
    let mut rows = stmt.query(params![area]).unwrap();
    while let Some(row) = rows.next().unwrap() {
        let name: String = row.get(0).unwrap();
        let run_id: i64 = row.get(1).unwrap();
        by_area.entry(name).or_default().insert(
            run_id,
            AreaScore {
                tests_pass: row.get(2).unwrap(),
                tests_total: row.get(3).unwrap(),
                subtests_pass: row.get(4).unwrap(),
                subtests_total: row.get(5).unwrap(),
                interop_score_sum: row.get::<_, i64>(6).unwrap() as u64,
            },
        );
    }
    let mut children: Vec<(String, Vec<Option<AreaScore>>)> = by_area
        .into_iter()
        .map(|(name, by_run)| {
            let scores = run_ids.iter().map(|id| by_run.get(id).copied()).collect();
            (name, scores)
        })
        .collect();
    match sort {
        AreaSort::Subtests => {
            let subtest_total = |scores: &[Option<AreaScore>]| {
                scores
                    .iter()
                    .flatten()
                    .map(|s| s.subtests_total)
                    .max()
                    .unwrap_or(0)
            };
            children.sort_by(|(a_name, a), (b_name, b)| {
                subtest_total(b)
                    .cmp(&subtest_total(a))
                    .then_with(|| a_name.cmp(b_name))
            });
        }
        AreaSort::Alpha => children.sort_by(|(a, _), (b, _)| a.cmp(b)),
    }
    children
}

#[derive(Clone, PartialEq)]
pub struct TestRow {
    pub name: String,
    /// The cross-engine union subtest denominator
    pub denom: u32,
    /// Per run: None if the run didn't run the test
    pub results: Vec<Option<TestRunResult>>,
}

#[derive(Clone, Copy, PartialEq)]
pub struct TestRunResult {
    pub status: i64,
    pub subtest_pass: u32,
    pub subtest_total: u32,
}

/// Tests directly in `area` (not in child areas) with per-run results.
pub fn tests_in_area(conn: &Connection, run_ids: &[i64], area: &str) -> Vec<TestRow> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT t.name, r.run_id, r.status, r.subtest_pass, r.subtest_total
             FROM tests t JOIN results r ON r.test_id = t.id
             WHERE t.area_id = (SELECT id FROM areas WHERE name = ?1)
             ORDER BY t.name",
        )
        .unwrap();
    let mut rows_by_test: Vec<(String, HashMap<i64, TestRunResult>)> = Vec::new();
    let mut rows = stmt.query(params![area]).unwrap();
    while let Some(row) = rows.next().unwrap() {
        let name: String = row.get(0).unwrap();
        let run_id: i64 = row.get(1).unwrap();
        let result = TestRunResult {
            status: row.get(2).unwrap(),
            subtest_pass: row.get(3).unwrap(),
            subtest_total: row.get(4).unwrap(),
        };
        match rows_by_test.last_mut() {
            Some((last_name, by_run)) if *last_name == name => {
                by_run.insert(run_id, result);
            }
            _ => {
                rows_by_test.push((name, HashMap::from([(run_id, result)])));
            }
        }
    }
    rows_by_test
        .into_iter()
        .map(|(name, by_run)| {
            let denom = by_run.values().map(|r| r.subtest_total).max().unwrap_or(1);
            let results = run_ids.iter().map(|id| by_run.get(id).copied()).collect();
            TestRow {
                name,
                denom,
                results,
            }
        })
        .collect()
}

#[derive(Clone, PartialEq)]
pub struct SubtestRow {
    pub name: String,
    /// Per run: None if the run didn't report the subtest
    pub statuses: Vec<Option<i64>>,
}

#[derive(Clone, PartialEq)]
pub struct TestDetail {
    pub name: String,
    /// Per-run top-level result
    pub results: Vec<Option<TestRunResult>>,
    pub subtests: Vec<SubtestRow>,
}

/// Full per-subtest comparison for a single test.
pub fn test_detail(conn: &Connection, run_ids: &[i64], test_name: &str) -> Option<TestDetail> {
    let test_id: i64 = conn
        .query_row(
            "SELECT id FROM tests WHERE name = ?1",
            params![test_name],
            |row| row.get(0),
        )
        .ok()?;

    let mut by_run: HashMap<i64, TestRunResult> = HashMap::new();
    {
        let mut stmt = conn
            .prepare_cached(
                "SELECT run_id, status, subtest_pass, subtest_total FROM results WHERE test_id = ?1",
            )
            .unwrap();
        let mut rows = stmt.query(params![test_id]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            by_run.insert(
                row.get(0).unwrap(),
                TestRunResult {
                    status: row.get(1).unwrap(),
                    subtest_pass: row.get(2).unwrap(),
                    subtest_total: row.get(3).unwrap(),
                },
            );
        }
    }

    let mut subtests: Vec<(i64, SubtestRow)> = Vec::new();
    {
        let mut stmt = conn
            .prepare_cached("SELECT id, name FROM subtests WHERE test_id = ?1 ORDER BY id")
            .unwrap();
        let mut rows = stmt.query(params![test_id]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            subtests.push((
                row.get(0).unwrap(),
                SubtestRow {
                    name: row.get(1).unwrap(),
                    statuses: vec![None; run_ids.len()],
                },
            ));
        }
    }
    let index_of: HashMap<i64, usize> = subtests
        .iter()
        .enumerate()
        .map(|(idx, (id, _))| (*id, idx))
        .collect();
    let run_index: HashMap<i64, usize> = run_ids
        .iter()
        .enumerate()
        .map(|(idx, id)| (*id, idx))
        .collect();
    {
        let mut stmt = conn
            .prepare_cached(
                "SELECT sr.subtest_id, sr.run_id, sr.status
                 FROM subtest_results sr JOIN subtests s ON s.id = sr.subtest_id
                 WHERE s.test_id = ?1",
            )
            .unwrap();
        let mut rows = stmt.query(params![test_id]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let subtest_id: i64 = row.get(0).unwrap();
            let run_id: i64 = row.get(1).unwrap();
            let status: i64 = row.get(2).unwrap();
            if let (Some(&sidx), Some(&ridx)) = (index_of.get(&subtest_id), run_index.get(&run_id))
            {
                subtests[sidx].1.statuses[ridx] = Some(status);
            }
        }
    }

    Some(TestDetail {
        name: test_name.to_string(),
        results: run_ids.iter().map(|id| by_run.get(id).copied()).collect(),
        subtests: subtests.into_iter().map(|(_, row)| row).collect(),
    })
}

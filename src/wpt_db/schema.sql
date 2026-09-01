PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS runs (
    id               INTEGER PRIMARY KEY,
    product          TEXT NOT NULL,
    browser_version  TEXT NOT NULL,
    os               TEXT,
    wpt_revision     TEXT NOT NULL,
    run_time         TEXT,
    source_run_id    INTEGER,
    ingested_at      TEXT NOT NULL,
    is_latest        INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS runs_source_run
    ON runs(product, source_run_id) WHERE source_run_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS runs_version_revision
    ON runs(product, browser_version, wpt_revision) WHERE source_run_id IS NULL;

CREATE TABLE IF NOT EXISTS areas (
    id        INTEGER PRIMARY KEY,
    name      TEXT NOT NULL UNIQUE,
    parent_id INTEGER REFERENCES areas(id)
);

CREATE TABLE IF NOT EXISTS tests (
    id      INTEGER PRIMARY KEY,
    name    TEXT NOT NULL UNIQUE,
    area_id INTEGER NOT NULL REFERENCES areas(id)
);
CREATE INDEX IF NOT EXISTS tests_area ON tests(area_id);

CREATE TABLE IF NOT EXISTS results (
    run_id        INTEGER NOT NULL REFERENCES runs(id),
    test_id       INTEGER NOT NULL REFERENCES tests(id),
    status        INTEGER NOT NULL,
    subtest_pass  INTEGER NOT NULL,
    subtest_total INTEGER NOT NULL,
    duration_ms   INTEGER,
    message       TEXT,
    PRIMARY KEY (run_id, test_id)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS subtests (
    id      INTEGER PRIMARY KEY,
    test_id INTEGER NOT NULL REFERENCES tests(id),
    name    TEXT NOT NULL,
    UNIQUE (test_id, name)
);

CREATE TABLE IF NOT EXISTS subtest_results (
    run_id     INTEGER NOT NULL REFERENCES runs(id),
    subtest_id INTEGER NOT NULL REFERENCES subtests(id),
    status     INTEGER NOT NULL,
    message    TEXT,
    PRIMARY KEY (run_id, subtest_id)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS area_scores (
    run_id            INTEGER NOT NULL REFERENCES runs(id),
    area_id           INTEGER NOT NULL REFERENCES areas(id),
    tests_pass        INTEGER NOT NULL,
    tests_total       INTEGER NOT NULL,
    subtests_pass     INTEGER NOT NULL,
    subtests_total    INTEGER NOT NULL,
    interop_score_sum INTEGER NOT NULL,
    PRIMARY KEY (run_id, area_id)
) WITHOUT ROWID;

-- Initial schema for docs/02-database-design.md, version 3.
-- Requires SQLite 3.38.0 or later with JSON functions enabled.
-- Run once against an empty database, outside an existing transaction.
-- Every application connection must also enable foreign_keys.

PRAGMA foreign_keys = ON;

BEGIN IMMEDIATE;

CREATE TABLE users (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    username_normalized TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('student', 'admin')),
    status TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
    created_at_ms INTEGER NOT NULL
) STRICT;

-- Publish by inserting a problem with a NULL pointer, inserting its version,
-- languages and cases, then updating the pointer in the same transaction.
CREATE TABLE problems (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    current_version_no INTEGER CHECK (current_version_no > 0),
    created_at_ms INTEGER NOT NULL,
    FOREIGN KEY (id, current_version_no)
        REFERENCES problem_versions (problem_id, version_no)
        ON DELETE RESTRICT ON UPDATE RESTRICT
) STRICT;

CREATE TABLE problem_versions (
    problem_id INTEGER NOT NULL,
    version_no INTEGER NOT NULL CHECK (version_no > 0),
    title TEXT NOT NULL,
    statement TEXT NOT NULL,
    comparison_mode TEXT NOT NULL DEFAULT 'trim_trailing_ascii_whitespace'
        CHECK (comparison_mode = 'trim_trailing_ascii_whitespace'),
    published_at_ms INTEGER NOT NULL,
    review_mode TEXT NOT NULL DEFAULT 'DISABLED'
        CHECK (review_mode IN ('DISABLED', 'ENABLE-ANY', 'ENABLE-ALL')),
    PRIMARY KEY (problem_id, version_no),
    FOREIGN KEY (problem_id) REFERENCES problems (id)
        ON DELETE RESTRICT ON UPDATE RESTRICT
) STRICT;

CREATE TABLE problem_version_languages (
    problem_id INTEGER NOT NULL,
    version_no INTEGER NOT NULL,
    language_id TEXT NOT NULL CHECK (language_id IN ('c', 'python')),
    cpu_time_ms INTEGER NOT NULL CHECK (cpu_time_ms > 0),
    wall_time_ms INTEGER NOT NULL CHECK (wall_time_ms > 0),
    memory_kib INTEGER NOT NULL CHECK (memory_kib > 0),
    PRIMARY KEY (problem_id, version_no, language_id),
    FOREIGN KEY (problem_id, version_no)
        REFERENCES problem_versions (problem_id, version_no)
        ON DELETE RESTRICT ON UPDATE RESTRICT
) STRICT;

CREATE TABLE test_cases (
    problem_id INTEGER NOT NULL,
    version_no INTEGER NOT NULL,
    case_index INTEGER NOT NULL CHECK (case_index > 0),
    input BLOB NOT NULL,
    expected_output BLOB NOT NULL,
    PRIMARY KEY (problem_id, version_no, case_index),
    FOREIGN KEY (problem_id, version_no)
        REFERENCES problem_versions (problem_id, version_no)
        ON DELETE RESTRICT ON UPDATE RESTRICT
) STRICT;

CREATE TABLE submissions (
    id TEXT PRIMARY KEY NOT NULL,
    user_id INTEGER NOT NULL,
    problem_id INTEGER NOT NULL,
    version_no INTEGER NOT NULL,
    language_id TEXT NOT NULL,
    source BLOB NOT NULL CHECK (length(source) > 0),
    create_request_key TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'dispatching', 'running', 'finished', 'failed')),
    verdict TEXT CHECK (verdict IN ('AC', 'WA', 'CE', 'RE', 'TLE', 'MLE', 'OLE')),
    terminal_details_json TEXT,
    created_at_ms INTEGER NOT NULL,
    terminal_at_ms INTEGER,
    review_status TEXT NOT NULL DEFAULT 'DISABLED'
        CHECK (review_status IN ('DISABLED', 'PENDING', 'DOUBTFUL', 'PASS', 'FAILED')),
    review_id TEXT,
    review_details_json TEXT
        CONSTRAINT submissions_review_json_object CHECK (
            CASE
                WHEN review_details_json IS NULL THEN 1
                WHEN json_valid(review_details_json)
                    THEN json_type(review_details_json) = 'object'
                ELSE 0
            END
        )
        CONSTRAINT submissions_review_fields CHECK (
            (review_status = 'DISABLED' AND review_id IS NULL AND review_details_json IS NULL)
            OR (review_status <> 'DISABLED' AND state = 'finished'
                AND review_id IS NOT NULL AND review_details_json IS NOT NULL)
        ),
    UNIQUE (user_id, create_request_key),
    FOREIGN KEY (user_id) REFERENCES users (id)
        ON DELETE RESTRICT ON UPDATE RESTRICT,
    FOREIGN KEY (problem_id, version_no, language_id)
        REFERENCES problem_version_languages (problem_id, version_no, language_id)
        ON DELETE RESTRICT ON UPDATE RESTRICT,
    -- Detailed result shapes and output limits are validated by the application.
    CONSTRAINT submissions_details_json_object CHECK (
        CASE
            WHEN terminal_details_json IS NULL THEN 1
            WHEN json_valid(terminal_details_json)
                THEN json_type(terminal_details_json) = 'object'
            ELSE 0
        END
    ),
    CONSTRAINT submissions_terminal_fields CHECK (
        (
            state IN ('queued', 'dispatching', 'running')
            AND verdict IS NULL
            AND terminal_details_json IS NULL
            AND terminal_at_ms IS NULL
        )
        OR (
            state = 'finished'
            AND verdict IS NOT NULL
            AND terminal_at_ms IS NOT NULL
            AND (
                (verdict = 'AC' AND terminal_details_json IS NULL)
                OR (verdict <> 'AC' AND terminal_details_json IS NOT NULL)
            )
        )
        OR (
            state = 'failed'
            AND verdict IS NULL
            AND terminal_details_json IS NOT NULL
            AND terminal_at_ms IS NOT NULL
        )
    )
) STRICT;

CREATE INDEX submissions_user_history_idx
    ON submissions (user_id, created_at_ms, id);

CREATE INDEX submissions_version_language_idx
    ON submissions (problem_id, version_no, language_id);

CREATE INDEX submissions_verdict_history_idx
    ON submissions (verdict, created_at_ms, id);

CREATE INDEX submissions_queued_order_idx
    ON submissions (created_at_ms, id) WHERE state = 'queued';

PRAGMA user_version = 3;

COMMIT;

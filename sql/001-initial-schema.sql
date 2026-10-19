-- Initial schema for docs/02-database-design.md, version 4.
-- Requires MySQL 8.4 and InnoDB. Run once against an empty database.
-- DDL implicitly commits; stop on the first error and reinitialize an empty DB.
-- The application does not create tables or upgrade existing databases.

SET NAMES utf8mb4 COLLATE utf8mb4_0900_bin;
SET SESSION sql_mode = 'STRICT_ALL_TABLES,NO_ENGINE_SUBSTITUTION';

CREATE TABLE schema_version (
    id BIGINT NOT NULL PRIMARY KEY CHECK (id = 1),
    version BIGINT NOT NULL CHECK (version > 0)
) ENGINE=InnoDB DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin;

CREATE TABLE users (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    username_normalized VARCHAR(255) NOT NULL UNIQUE,
    display_name LONGTEXT NOT NULL,
    password_hash LONGTEXT NOT NULL,
    role VARCHAR(32) NOT NULL CHECK (role IN ('student', 'admin')),
    status VARCHAR(32) NOT NULL CHECK (status IN ('active', 'disabled')),
    created_at_ms BIGINT NOT NULL
) ENGINE=InnoDB DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin;

-- Publish by inserting a problem with a NULL pointer, inserting its version,
-- languages and cases, then updating the pointer in the same transaction.
CREATE TABLE problems (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    current_version_no BIGINT CHECK (current_version_no > 0),
    created_at_ms BIGINT NOT NULL
) ENGINE=InnoDB DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin;

CREATE TABLE problem_versions (
    problem_id BIGINT NOT NULL,
    version_no BIGINT NOT NULL CHECK (version_no > 0),
    title LONGTEXT NOT NULL,
    statement LONGTEXT NOT NULL,
    comparison_mode VARCHAR(64) NOT NULL DEFAULT 'trim_trailing_ascii_whitespace'
        CHECK (comparison_mode = 'trim_trailing_ascii_whitespace'),
    published_at_ms BIGINT NOT NULL,
    review_mode VARCHAR(32) NOT NULL DEFAULT 'DISABLED'
        CHECK (review_mode IN ('DISABLED', 'ENABLE-ANY', 'ENABLE-ALL')),
    PRIMARY KEY (problem_id, version_no),
    FOREIGN KEY (problem_id) REFERENCES problems (id)
        ON DELETE RESTRICT ON UPDATE RESTRICT
) ENGINE=InnoDB DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin;

CREATE TABLE problem_version_languages (
    problem_id BIGINT NOT NULL,
    version_no BIGINT NOT NULL,
    language_id VARCHAR(16) NOT NULL CHECK (language_id IN ('c', 'python')),
    cpu_time_ms BIGINT NOT NULL CHECK (cpu_time_ms > 0),
    wall_time_ms BIGINT NOT NULL CHECK (wall_time_ms > 0),
    memory_kib BIGINT NOT NULL CHECK (memory_kib > 0),
    PRIMARY KEY (problem_id, version_no, language_id),
    FOREIGN KEY (problem_id, version_no)
        REFERENCES problem_versions (problem_id, version_no)
        ON DELETE RESTRICT ON UPDATE RESTRICT
) ENGINE=InnoDB DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin;

CREATE TABLE test_cases (
    problem_id BIGINT NOT NULL,
    version_no BIGINT NOT NULL,
    case_index BIGINT NOT NULL CHECK (case_index > 0),
    input LONGBLOB NOT NULL,
    expected_output LONGBLOB NOT NULL,
    PRIMARY KEY (problem_id, version_no, case_index),
    FOREIGN KEY (problem_id, version_no)
        REFERENCES problem_versions (problem_id, version_no)
        ON DELETE RESTRICT ON UPDATE RESTRICT
) ENGINE=InnoDB DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin;

CREATE TABLE submissions (
    id VARCHAR(36) NOT NULL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    problem_id BIGINT NOT NULL,
    version_no BIGINT NOT NULL,
    language_id VARCHAR(16) NOT NULL,
    source LONGBLOB NOT NULL CHECK (OCTET_LENGTH(source) > 0),
    create_request_key VARCHAR(36) NOT NULL,
    state VARCHAR(32) NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'dispatching', 'running', 'finished', 'failed')),
    verdict VARCHAR(32) CHECK (verdict IN ('AC', 'WA', 'CE', 'RE', 'TLE', 'MLE', 'OLE')),
    terminal_details_json LONGTEXT,
    created_at_ms BIGINT NOT NULL,
    terminal_at_ms BIGINT,
    review_status VARCHAR(32) NOT NULL DEFAULT 'DISABLED'
        CHECK (review_status IN ('DISABLED', 'PENDING', 'DOUBTFUL', 'PASS', 'FAILED')),
    review_id VARCHAR(36),
    review_details_json LONGTEXT,
    CONSTRAINT submissions_review_json_object CHECK (
        CASE
            WHEN review_details_json IS NULL THEN 1
            WHEN JSON_VALID(review_details_json)
                THEN JSON_TYPE(review_details_json) = 'OBJECT'
            ELSE 0
        END = 1
    ),
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
            WHEN JSON_VALID(terminal_details_json)
                THEN JSON_TYPE(terminal_details_json) = 'OBJECT'
            ELSE 0
        END = 1
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
) ENGINE=InnoDB DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin;

-- Add the publication pointer after its referenced table exists.
ALTER TABLE problems ADD CONSTRAINT problems_current_version_fk
    FOREIGN KEY (id, current_version_no)
        REFERENCES problem_versions (problem_id, version_no)
        ON DELETE RESTRICT ON UPDATE RESTRICT;

CREATE INDEX submissions_user_history_idx
    ON submissions (user_id, created_at_ms, id);

CREATE INDEX submissions_version_language_idx
    ON submissions (problem_id, version_no, language_id);

CREATE INDEX submissions_verdict_history_idx
    ON submissions (verdict, created_at_ms, id);

CREATE INDEX submissions_queued_order_idx
    ON submissions (state, created_at_ms, id);

-- Write the version marker only after every DDL statement has succeeded.
INSERT INTO schema_version (id, version) VALUES (1, 4);

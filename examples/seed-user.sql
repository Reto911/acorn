-- Development-only test owner; run once after sql/001-initial-schema.sql.
-- The Argon2id PHC hash was derived from discarded random password bytes.
-- No usable login credential is provided; authentication is not implemented.

SET NAMES utf8mb4 COLLATE utf8mb4_0900_bin;
SET SESSION sql_mode = 'STRICT_ALL_TABLES,NO_ENGINE_SUBSTITUTION';

START TRANSACTION;

INSERT INTO users (
    id, username_normalized, display_name, password_hash, role, status, created_at_ms
)
VALUES (
    1,
    'test',
    'Test User',
    '$argon2id$v=19$m=19456,t=2,p=1$/ld7GeCDZtHFUJg4erTMpg$qZtWznNPZaqvopjbq4PFUxkTKYoOlJeKRGspUkODgSY',
    'student',
    'active',
    CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED)
);

COMMIT;

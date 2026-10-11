-- Built-in example problem retained in release distributions.
-- Run once after 001-initial-schema.sql against a newly initialized database.
-- This is initial data, not a schema migration. It does not create a user.

PRAGMA foreign_keys = ON;

BEGIN IMMEDIATE;

INSERT INTO problems (id, current_version_no, created_at_ms)
VALUES (1, NULL, CAST(strftime('%s', 'now') AS INTEGER) * 1000);

INSERT INTO problem_versions (
    problem_id, version_no, title, statement, comparison_mode, published_at_ms
)
VALUES (
    1,
    1,
    '两数求和（A + B）',
    '给定两个整数 a 和 b，输出它们的和。

## 输入格式

一行两个以空格分隔的整数 a 和 b，满足 -1000000000 <= a, b <= 1000000000。

## 输出格式

输出一个整数，表示 a + b，并换行。

## 样例输入

```text
1 2
```

## 样例输出

```text
3
```',
    'trim_trailing_ascii_whitespace',
    CAST(strftime('%s', 'now') AS INTEGER) * 1000
);

INSERT INTO problem_version_languages (
    problem_id, version_no, language_id, cpu_time_ms, wall_time_ms, memory_kib
)
VALUES
    (1, 1, 'c', 1000, 3000, 262144),
    (1, 1, 'python', 2000, 5000, 262144);

INSERT INTO test_cases (
    problem_id, version_no, case_index, input, expected_output
)
VALUES
    (1, 1, 1, CAST('1 2' || char(10) AS BLOB), CAST('3' || char(10) AS BLOB)),
    (1, 1, 2, CAST('0 0' || char(10) AS BLOB), CAST('0' || char(10) AS BLOB)),
    (1, 1, 3, CAST('-3 5' || char(10) AS BLOB), CAST('2' || char(10) AS BLOB)),
    (1, 1, 4, CAST('-7 -11' || char(10) AS BLOB), CAST('-18' || char(10) AS BLOB));

UPDATE problems SET current_version_no = 1 WHERE id = 1;

COMMIT;

# Acorn

English | [简体中文](README_zh-CN.md)

Acorn is a programming judge for single-file C and Python 3 submissions, with a browser editor, isolated execution, submission history, and LLM code review.

## Features

- Browse problems, write C or Python code in Monaco, and save local drafts by problem and language.
- Compile, execute, and compare output in Isolate sandboxes, with limits on CPU time, wall time, memory, process count, output, and disk usage.
- Process different submissions in parallel across multiple Workers, saving verdicts, compiler diagnostics, and output from the first failing test case.
- Filter submission history by problem, execution state, verdict, and review status. Historical results reference the problem version used for judging.
- Trigger LLM reviews automatically according to a problem's policy, or start and rerun reviews manually to check for hardcoding, guessing, and attempts to bypass problem requirements.
- View backend admission conditions, the scheduling queue, and recent database and Worker observations on the system status page.

## Project Structure

The backend serves the browser over HTTP JSON and publishes jobs and consumes Worker events through Kafka; gRPC provides read-only Worker status. Workers execute code in Isolate, while the backend stores submissions and results in MySQL. LLM reviews run through a separate queue in the backend.

| Directory                                    | Contents                                                                     |
| -------------------------------------------- | ---------------------------------------------------------------------------- |
| [crates/backend](crates/backend/README.md)   | Rust backend, HTTP API, Scheduler, health collection, and LLM reviews        |
| [crates/worker](crates/worker/README.md)     | C/Python judging executor, Isolate wrapper, Kafka consumption and status RPC |
| [crates/protocol](crates/protocol/README.md) | Protobuf messages and generated gRPC clients and servers                     |
| [crates/kafka](crates/kafka/README.md)       | Shared Kafka defaults, validation, and client configuration policy           |
| [web](web/README.md)                         | Browser interface built with Vue 3, TypeScript, Element Plus, and Monaco     |
| [sql](sql/README.md)                         | MySQL schema and the built-in A + B problem                                  |
| [config](config)                             | Example TOML configurations for the backend and Worker                       |
| [examples](examples)                         | Development user seed SQL and sample submissions                             |
| [docs](docs/README.md)                       | Architecture, interface contracts, and development records                   |

## Installation

### Requirements

| Dependency                                                      | Purpose                                                               |
| --------------------------------------------------------------- | --------------------------------------------------------------------- |
| Linux, Isolate, cgroups, and filesystem user quotas             | Isolated execution and resource limits                                |
| GCC and Python 3                                                | C compilation and Python execution                                    |
| Rust/Cargo 1.94.0 or later                                      | Build the Rust 2024 edition workspace and its locked dependencies     |
| `protoc` with proto3 optional support                           | Generate the shared gRPC protocol                                     |
| librdkafka development package (at least 2.12.1) and pkg-config | Default dynamic build; provide the matching shared library at runtime |
| MySQL 8.4 / InnoDB and the MySQL command-line client            | Initialize and manage the database                                    |
| Node.js 24.x, at least 24.21.0; pnpm 12.11.2                    | Install, build, and run the frontend                                  |
| Kafka broker reachable by the backend and Workers               | Task and event topics with initialized consumer group offsets         |
| curl                                                            | Call the HTTP API from the command line                               |

Have an administrator prepare Isolate, cgroups, user quotas, and execution permissions using the [Worker installation guide](crates/worker/README.md#安装与沙箱准备). The paths to GCC, Python, and Isolate are configured in the Worker TOML file.

`isolate --check-config` validates Isolate configuration; filesystem user quotas must be enabled separately. A dedicated ext4 filesystem can be prepared using the [quota setup example](crates/worker/README.md#用户配额文件系统示例).

### Build

From the repository root, run:

```sh
cargo build --workspace --locked
```

The executables are `target/debug/acorn-worker` and `target/debug/acorn-backend`. `protoc` is resolved from PATH, or from the `PROTOC` environment variable.

The default `kafka-dynamic` feature uses the system librdkafka through pkg-config. To build the bundled librdkafka statically, install a C/C++ toolchain, CMake, make or Ninja, and pkg-config, then run:

```sh
cargo build --workspace --locked --no-default-features --features acorn-backend/kafka-static,acorn-worker/kafka-static
```

Static linking here applies to librdkafka. Select the same link mode for the backend and Worker in each workspace build.

Install frontend dependencies:

```sh
cd web
pnpm install --frozen-lockfile
cp .env.example .env.local
```

## Quick Start

This workflow starts one Worker, one backend, and a frontend development server locally on the default ports. Run backend and Worker commands from the repository root.

### Prepare Configuration

```sh
mkdir -p "$HOME/.config/acorn"
cp config/worker.example.toml "$HOME/.config/acorn/worker.toml"
cp config/backend.example.toml "$HOME/.config/acorn/backend.toml"
```

Edit `worker.toml` to check `sandbox.executable`, `languages.c.compiler`, and `languages.python.interpreter`. Choose an available `sandbox.box_id` dedicated to this Worker on the host. The example backend configuration connects to `http://127.0.0.1:50051` and selects the MySQL database configured by `database.endpoint`, `name`, and `username`.

### Initialize the Database

Have a deployment administrator create an empty `appdb` database. Use an initialization account with DDL/DML permissions to run the following commands once; each command prompts for its password. The backend account only needs business read/write permissions.

```sh
mysql --host=127.0.0.1 --port=3306 --user=example_initializer --password --ssl-mode=DISABLED --database=appdb < sql/001-initial-schema.sql
mysql --host=127.0.0.1 --port=3306 --user=example_initializer --password --ssl-mode=DISABLED --database=appdb < examples/seed-user.sql
mysql --host=127.0.0.1 --port=3306 --user=example_initializer --password --ssl-mode=DISABLED --database=appdb < sql/002-example-problem.sql
```

This creates schema version 4, user 1, and the A + B problem with ID 1, C/Python resource limits, and 104 test cases. Reuse this database on subsequent starts. Initialize an empty database and stop at the first SQL error. The backend validates the schema version and configured user at startup. The initialization scripts use MySQL syntax; the application does not import old databases or upgrade them automatically.

See the [SQL guide](sql/README.md) for script contents, account responsibilities, and handling partial initialization.

### Prepare Kafka

Configure a reachable broker, create the task and event topics, and initialize both consumer groups as specified in the [deployment contract](docs/10-kafka-judging-design.md#10-整体部署). Match the topics in both TOML files and use separate backend-event and Worker-task groups. All Workers share the task group. The services disable automatic topic creation and offset reset.

### Start the Services

Start the Worker in terminal A:

```sh
target/debug/acorn-worker --config "$HOME/.config/acorn/worker.toml"
```

Start the backend in terminal B:

```sh
read -r -s -p 'MySQL password: ' MYSQL_PASSWORD
printf '\n'
export MYSQL_PASSWORD
target/debug/acorn-backend --config "$HOME/.config/acorn/backend.toml"
```

Start the frontend in terminal C:

```sh
cd web
pnpm dev
```

### Submit Your First Program

Open the [problem list](http://localhost:5173/problems), select the A + B problem (`两数求和（A + B）`), choose C or Python 3, paste the [C sample](examples/submissions/a-plus-b.c) or [Python sample](examples/submissions/a-plus-b.py), and click Submit code (`提交代码`). The result page polls automatically; the correct samples receive `AC`. Open Submission history (`提交历史`) to revisit results.

You can also submit over HTTP:

```sh
ACORN_REQUEST_KEY=$(python3 -c 'import uuid; print(uuid.uuid4())')
curl -i http://127.0.0.1:8080/api/v1/submissions \
  -H 'Content-Type: application/json' \
  -H "Idempotency-Key: $ACORN_REQUEST_KEY" \
  -d '{"problem_id":1,"language_id":"python","source":"a,b=map(int,input().split());print(a+b)\n"}'
```

The response returns `202`, a `submission_id`, and a query URL in `Location`. Insert the returned ID below and repeat the query until `state` is `finished` or `failed`:

```sh
ACORN_SUBMISSION_ID='<submission_id from the response>'
curl --fail-with-body "http://127.0.0.1:8080/api/v1/submissions/$ACORN_SUBMISSION_ID"
```

Keep the original idempotency key and content when resending the same request. Generate a new key to create a new submission. More examples are available in the [backend guide](crates/backend/README.md#http-api).

## Configuration

Both the backend and Worker read TOML through `--config <PATH>`; restart the service after changes. All submissions belong to the preconfigured user selected by the backend's `test_user.user_id`. HTTP and gRPC have no built-in authentication; restrict access through a reverse proxy or firewall.

| Setting                                   | Example value                | Purpose                                            |
| ----------------------------------------- | ---------------------------- | -------------------------------------------------- |
| Worker `server.listen`                    | `127.0.0.1:50051`            | gRPC listening address                             |
| Worker `sandbox.box_id`                   | `0`                          | Isolate box ID reserved on this host               |
| Backend `http.listen`                     | `127.0.0.1:8080`             | HTTP API listening address                         |
| Backend `database.endpoint`               | `127.0.0.1:3306`             | MySQL host and TCP port                            |
| Backend `database.name`                   | `appdb`                      | Initialized schema version 4 database              |
| Backend `database.username`               | `example_user`               | Business read/write account                        |
| Backend `MYSQL_PASSWORD`                  | Process environment          | Database password                                  |
| Backend `database.max_connections`        | `4`                          | Shared application pool ceiling                    |
| Backend `database.acquire_timeout_ms`     | `5000`                       | Entire connection acquisition budget               |
| Backend `database.lock_wait_timeout_secs` | `3`                          | Session row and metadata lock wait                 |
| Backend `scheduler.workers`               | `["http://127.0.0.1:50051"]` | Worker endpoint list                               |
| Backend `scheduler.pending_max_count`     | `128`                        | Capacity for submissions awaiting Worker admission |
| Backend `llm_review.enabled`              | `false`                      | Enable LLM reviews                                 |
| Frontend `API_PROXY_TARGET`               | `http://127.0.0.1:8080`      | API proxy target for the development server        |
| Frontend `VITE_SOURCE_MAX_BYTES`          | `65536`                      | Source byte limit checked before submission        |

The backend reads `MYSQL_PASSWORD` from its process environment and uses MySQL with SSL disabled. It does not load `.env` automatically. Connections use utf8mb4_0900_bin, READ COMMITTED, strict SQL mode, and finite lock waits.

Align request, message, and result budgets between the backend and all Workers. Field suffixes `_ms`, `_kib`, and `_bytes` indicate units. See the [backend example](config/backend.example.toml), [Worker example](config/worker.example.toml), and [frontend configuration guide](web/README.md#配置与-api-代理) for the complete settings.

Both backend and Worker configurations require `[kafka]` with bootstrap endpoints, both topics, and `group_id` for their own consumer group; budgets have defaults. The backend also provides independent `[worker_membership]` observation settings. See the [backend guide](crates/backend/README.md#kafka-客户端配置), [Worker guide](crates/worker/README.md#kafka-客户端配置), and [protocol guide](crates/protocol/README.md#kafka-消息编解码) for fields, client options, and message codecs. For configurations using the earlier Kafka field names, rename `[kafka].events_group_id` (backend) or `[kafka].tasks_group_id` (Worker) to `group_id`, preserving its value. For existing configurations, merge the example `[kafka]` section into the file passed to `--config`, preserving database, review, and other settings. `[worker_membership]` may be omitted to use its defaults.

Existing deployment files must remove `scheduler.poll_interval_ms`, `grpc.submit_timeout_ms/get_timeout_ms/ack_timeout_ms`, Worker `results.acknowledged_queue_length`, and Kafka `commit_timeout_ms`.

### Multiple Workers

Give each Worker its own listening address and a `sandbox.box_id` reserved on its host, share the Kafka task group across Workers, then add their status endpoints to the backend configuration:

```toml
[scheduler]
pending_max_count = 128
workers = ["http://127.0.0.1:50051", "http://127.0.0.1:50052"]
retry_interval_ms = 1_000
```

Each Worker executes one submission at a time. Multiple Workers process different submissions in parallel.

### LLM Code Review

Set `enabled = true` in the backend's `[llm_review]` section and provide the complete Responses API `endpoint`, `api_key`, and `model`. Store the key in the server-side TOML file. After restarting, use Start review (`启动审查`) or Rerun review (`重新审查`) on the result page of a completed judging run.

Automatic reviews follow the `review_mode` of the submission's fixed problem version: `DISABLED` turns them off, `ENABLE-ANY` triggers after at least one test case passes, and `ENABLE-ALL` triggers when all cases pass. The built-in example problem disables automatic reviews by default. Review opinions are saved separately; rerunning replaces the previous opinion. See the [backend review guide](crates/backend/README.md#llm-代码审查) for configuration and HTTP operations.

## Operation and Deployment

The System status (`系统状态`) page at [/health](http://localhost:5173/health) fetches a snapshot every 10 seconds and supports manual refresh. From the command line, use `GET /api/v1/health`.

The snapshot includes database reads, metadata checks for both Kafka topics, and Worker execution and pending MQ handoff counts. Health checks do not gate admission. Independent Worker group observation rejects new records with `503 WorkersUnavailable` until a fresh positive member count is available; existing idempotency keys retain their normal behavior. Configure `[worker_membership].tasks_group_id` to match the Workers’ `[kafka].group_id`.

Both Rust services write English logs to stderr, with the level controlled by `logging.level`. Use submission IDs to correlate backend dispatch, result persistence, and Worker execution logs.

To stop the system, send Ctrl-C or SIGTERM to the backend first. The backend drains registered local operations and current message work within its message budget; it does not wait for remote judging to finish. Workers stop through their sandbox and message shutdown budgets. On restart, only queued submissions are republished; dispatching/running submissions continue waiting for events. Old pending reviews are interrupted. Consumer RAII destruction runs on a blocking thread and may continue after the application wait times out. Persisted terminal results remain available.

For frontend deployment, build `web/dist/`, serve it with a static server, and proxy same-origin `/api/` requests to the backend. Use HTTPS for remote browser access to support `crypto.randomUUID()` when creating submissions. See the [frontend deployment guide](web/README.md#构建与静态部署) for an example.

## Development Checks

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p acorn-backend --locked --lib --test config_results
```

The workspace requires Rustdoc for authored public and private Rust items, and each crate inherits the shared lint configuration. See the module READMEs for tests, protocol generation, and frontend checks, and the [documentation index](docs/README.md) for architecture and interface details.

Database and workflow checks require `MYSQL_PASSWORD` and a private copy of [the MySQL test configuration](config/backend-test.example.toml), selected by `ACORN_TEST_DATABASE_CONFIG`. The development account creates and deletes a separate database for each test; see the [backend checks](crates/backend/README.md#检查命令).

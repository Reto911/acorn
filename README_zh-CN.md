# Acorn

[English](README.md) | 简体中文

Acorn 是支持 C 和 Python 3 的单文件源码判题系统，提供浏览器编辑器、隔离执行、提交历史和 LLM 代码审查。

## 功能

- 浏览题目，在 Monaco 编辑器中编写 C 或 Python 代码，按题目和语言保存本地草稿。
- 在 Isolate 沙箱中编译、执行并比较输出，支持 CPU 时间、墙钟时间、内存、进程数、输出和磁盘限额。
- 通过多个 Worker 并行处理不同提交，保存判题结果、编译诊断和首个失败用例的输出。
- 查询提交历史，按题目、执行状态、判题结论和审查状态筛选；历史结果关联实际判题的题目版本。
- 按题目策略自动调用 LLM，或手动启动、重新审查，查看针对硬编码、猜测和规避实现的审查意见。
- 在系统状态页面查看后端接纳条件、调度队列、数据库和 Worker 的最近观测。

## 项目结构

后端通过 HTTP JSON 服务浏览器，通过 Kafka 发布任务并消费 Worker 事件，gRPC 仅提供 Worker 状态观测；Worker 在 Isolate 中执行代码，后端将提交及结果保存到 MySQL。LLM 审查由后端的独立队列执行。

| 目录                                         | 内容                                                    |
| -------------------------------------------- | ------------------------------------------------------- |
| [crates/backend](crates/backend/README.md)   | Rust 后端、HTTP API、Scheduler、健康采集和 LLM 审查     |
| [crates/worker](crates/worker/README.md)     | C/Python 判题执行器、Isolate 封装、Kafka 消费和状态 RPC |
| [crates/protocol](crates/protocol/README.md) | Protobuf 消息及生成的 gRPC 客户端、服务端               |
| [crates/kafka](crates/kafka/README.md)       | Kafka 配置默认值、通用校验和共享客户端策略              |
| [web](web/README.md)                         | Vue 3、TypeScript、Element Plus 和 Monaco 浏览器界面    |
| [sql](sql/README.md)                         | MySQL 建表脚本及内置两数求和题                          |
| [config](config)                             | 后端和 Worker 的 TOML 配置示例                          |
| [examples](examples)                         | 开发用户预置 SQL 和提交源码样例                         |
| [docs](docs/README.md)                       | 架构设计、接口合同和开发记录                            |

## 安装

### 运行环境

| 依赖                                          | 用途                                   |
| --------------------------------------------- | -------------------------------------- |
| Linux、Isolate、控制组和文件系统用户配额      | 隔离执行及资源限制                     |
| GCC、Python 3                                 | C 编译和 Python 执行                   |
| Rust/Cargo 1.94.0 或更新版本                  | 构建 Rust 2024 edition 工程及锁定依赖  |
| 支持 proto3 optional 的 `protoc`              | 构建共享 gRPC 协议                     |
| librdkafka 开发包（至少 2.12.1）和 pkg-config | 默认动态构建；运行环境提供对应共享库   |
| MySQL 8.4／InnoDB 及 MySQL 命令行客户端       | 初始化和管理数据库                     |
| Node.js 24.x，至少 24.21.0；pnpm 12.11.2      | 安装、构建和运行前端                   |
| 后端与 Worker 均可访问的 Kafka broker         | 任务及事件 topic、已初始化的消费组位点 |
| curl                                          | 从命令行调用 HTTP API                  |

先由管理员按 [Worker 安装说明](crates/worker/README.md#安装与沙箱准备)准备 Isolate、控制组、用户配额和执行权限。Worker 使用的 GCC、Python 和 Isolate 路径在 TOML 中指定。

`isolate --check-config` 检查 Isolate 配置有效性，文件系统用户配额需要单独启用。可按[配额准备示例](crates/worker/README.md#用户配额文件系统示例)准备专用 ext4 文件系统。

### 构建

在仓库根目录执行：

```sh
cargo build --workspace --locked
```

可执行文件为 `target/debug/acorn-worker` 和 `target/debug/acorn-backend`。`protoc` 从 PATH 查找，也可通过 `PROTOC` 环境变量指定。

默认启用 `kafka-dynamic`，通过 pkg-config 使用系统 librdkafka。显式静态构建需要 C/C++ 工具链、CMake、make 或 Ninja 和 pkg-config，由 Cargo 构建随附的 librdkafka 源码：

```sh
cargo build --workspace --locked --no-default-features --features acorn-backend/kafka-static,acorn-worker/kafka-static
```

静态仅指 librdkafka；同一次 workspace 构建为后端和 Worker 选择一致的链接方式。

安装前端依赖：

```sh
cd web
pnpm install --frozen-lockfile
cp .env.example .env.local
```

## Quick Start

以下流程使用默认端口，在本机启动一个 Worker、一个后端和前端开发服务。后端与 Worker 的命令均从仓库根目录执行。

### 准备配置

```sh
mkdir -p "$HOME/.config/acorn"
cp config/worker.example.toml "$HOME/.config/acorn/worker.toml"
cp config/backend.example.toml "$HOME/.config/acorn/backend.toml"
```

编辑 `worker.toml`，核对 `sandbox.executable`、`languages.c.compiler` 和 `languages.python.interpreter`，为 `sandbox.box_id` 选择本机空闲且专用于该 Worker 的 ID。后端示例配置连接 `http://127.0.0.1:50051`，数据库由 `database.endpoint`、`name` 和 `username` 指定。

### 初始化数据库

先由部署管理员创建空数据库 `appdb`，再使用具备 DDL／DML 权限的初始化账户执行以下命令，每条命令会提示输入密码。后端账户只需业务读写权限。

```sh
mysql --host=127.0.0.1 --port=3306 --user=example_initializer --password --ssl-mode=DISABLED --database=appdb < sql/001-initial-schema.sql
mysql --host=127.0.0.1 --port=3306 --user=example_initializer --password --ssl-mode=DISABLED --database=appdb < examples/seed-user.sql
mysql --host=127.0.0.1 --port=3306 --user=example_initializer --password --ssl-mode=DISABLED --database=appdb < sql/002-example-problem.sql
```

这会创建结构版本 4、用户 1 和题号 1 的两数求和题，包含 C/Python 限额与 104 个测试用例。后续启动直接复用该数据库。初始化针对空库，遇到首个 SQL 错误即停止；后端启动检查结构版本和配置用户。初始化脚本使用 MySQL 方言，应用不导入旧库或自动升级。

脚本内容、账户职责和初始化中途失败的处理见[SQL 说明](sql/README.md)。

### 准备 Kafka

配置可互通的 broker，按[部署合同](docs/10-kafka-judging-design.md#10-整体部署)创建任务及事件 topic 并初始化两个消费组位点。两份 TOML 的 topic 保持一致，后端事件组和 Worker 任务组分开；所有 Worker 共用任务组。服务禁止自动建 topic 和自动重置位点。

### 启动服务

终端 A 启动 Worker：

```sh
target/debug/acorn-worker --config "$HOME/.config/acorn/worker.toml"
```

终端 B 启动后端：

```sh
read -r -s -p 'MySQL 密码：' MYSQL_PASSWORD
printf '\n'
export MYSQL_PASSWORD
target/debug/acorn-backend --config "$HOME/.config/acorn/backend.toml"
```

终端 C 启动前端：

```sh
cd web
pnpm dev
```

### 提交第一份代码

打开[题目列表](http://localhost:5173/problems)，进入“两数求和（A + B）”，选择 C 或 Python 3，粘贴 [C 样例](examples/submissions/a-plus-b.c)或 [Python 样例](examples/submissions/a-plus-b.py)，点击“提交代码”。结果页自动轮询，正确样例得到 `AC`；“提交历史”可以重新打开结果。

也可以通过 HTTP 提交：

```sh
ACORN_REQUEST_KEY=$(python3 -c 'import uuid; print(uuid.uuid4())')
curl -i http://127.0.0.1:8080/api/v1/submissions \
  -H 'Content-Type: application/json' \
  -H "Idempotency-Key: $ACORN_REQUEST_KEY" \
  -d '{"problem_id":1,"language_id":"python","source":"a,b=map(int,input().split());print(a+b)\n"}'
```

响应返回 `202`、`submission_id` 和查询地址 `Location`。将返回的 ID 填入下述命令，重复查询直到 `state` 为 `finished` 或 `failed`：

```sh
ACORN_SUBMISSION_ID='<响应中的 submission_id>'
curl --fail-with-body "http://127.0.0.1:8080/api/v1/submissions/$ACORN_SUBMISSION_ID"
```

重复发送同一次请求时保留原幂等键和内容，主动创建新提交时生成新键。更多调用示例见[后端说明](crates/backend/README.md#http-api)。

## 配置

后端与 Worker 均通过 `--config <PATH>` 读取 TOML，修改后重启生效。所有提交归属于后端 `test_user.user_id` 指定的预置用户。HTTP 和 gRPC 未内置鉴权，部署时通过反向代理或防火墙限制访问。

| 配置项                                 | 示例值                       | 用途                        |
| -------------------------------------- | ---------------------------- | --------------------------- |
| Worker `server.listen`                 | `127.0.0.1:50051`            | gRPC 监听地址               |
| Worker `sandbox.box_id`                | `0`                          | 本机独占的 Isolate box ID   |
| 后端 `http.listen`                     | `127.0.0.1:8080`             | HTTP API 监听地址           |
| 后端 `database.endpoint`               | `127.0.0.1:3306`             | MySQL 主机和 TCP 端口       |
| 后端 `database.name`                   | `appdb`                      | 已初始化的结构版本 4 数据库 |
| 后端 `database.username`               | `example_user`               | 业务读写账户                |
| 后端 `MYSQL_PASSWORD`                  | 进程环境变量                 | 数据库密码                  |
| 后端 `database.max_connections`        | `4`                          | 共享应用连接池上限          |
| 后端 `database.acquire_timeout_ms`     | `5000`                       | 整个连接取得过程的预算      |
| 后端 `database.lock_wait_timeout_secs` | `3`                          | 会话行锁与元数据锁等待      |
| 后端 `scheduler.workers`               | `["http://127.0.0.1:50051"]` | Worker 地址列表             |
| 后端 `scheduler.pending_max_count`     | `128`                        | 等待 Worker 接纳的提交容量  |
| 后端 `llm_review.enabled`              | `false`                      | LLM 审查开关                |
| 前端 `API_PROXY_TARGET`                | `http://127.0.0.1:8080`      | 开发服务的 API 代理目标     |
| 前端 `VITE_SOURCE_MAX_BYTES`           | `65536`                      | 提交源码字节上限预检        |

后端从进程环境读取 `MYSQL_PASSWORD`，MySQL SSL 显式禁用，程序不自动加载 `.env`。连接使用 utf8mb4_0900_bin、READ COMMITTED、严格 SQL 模式和有限锁等待。

请求、消息和结果预算需要在后端与各 Worker 之间对齐。字段以 `_ms`、`_kib`、`_bytes` 标明单位。完整配置见[后端示例](config/backend.example.toml)、[Worker 示例](config/worker.example.toml)和[前端配置说明](web/README.md#配置与-api-代理)。

后端与 Worker 的配置均要求 `[kafka]`，包含端点、两个 topic 和标识各自消费组的 `group_id`；预算可使用默认值。后端另有独立的 `[worker_membership]` 采集配置。字段、客户端选项与消息编解码接口见[后端说明](crates/backend/README.md#kafka-客户端配置)、[Worker 说明](crates/worker/README.md#kafka-客户端配置)和[协议说明](crates/protocol/README.md#kafka-消息编解码)。 已使用早先 Kafka 配置的，将后端 `[kafka].events_group_id` 或 Worker `[kafka].tasks_group_id` 改名为 `group_id`，保留原值。使用已有配置时，将示例中的 `[kafka]` 节合并到实际 `--config` 指定的文件，保留数据库及审查等已有配置；`[worker_membership]` 可省略并使用默认值。

已有部署文件须删除 `scheduler.poll_interval_ms`、`grpc.submit_timeout_ms/get_timeout_ms/ack_timeout_ms`、Worker 的 `results.acknowledged_queue_length` 和 Kafka 的 `commit_timeout_ms`。

### 多 Worker

为每个 Worker 分别配置监听地址和本机独占的 `sandbox.box_id`，所有 Worker 共用 Kafka 任务组，再将状态地址加入后端：

```toml
[scheduler]
pending_max_count = 128
workers = ["http://127.0.0.1:50051", "http://127.0.0.1:50052"]
retry_interval_ms = 1_000
```

每个 Worker 同时执行一个提交，多个 Worker 可以并行处理不同提交。

### LLM 代码审查

在后端 `[llm_review]` 中设置 `enabled = true`，填写完整的 Responses API `endpoint`、`api_key` 和 `model`。密钥保存在服务端 TOML 中。重启后，可在机器判题已完成的结果页点击“启动审查”或“重新审查”。

自动审查由提交固定题目版本的 `review_mode` 决定：`DISABLED` 关闭，`ENABLE-ANY` 在至少一例通过时触发，`ENABLE-ALL` 在全部通过时触发。内置示例题默认关闭自动审查。审查意见单独保存，重新审查会替换上次意见。配置和 HTTP 操作见[后端审查说明](crates/backend/README.md#llm-代码审查)。

## 运行与部署

“系统状态”页面位于 [/health](http://localhost:5173/health)，每 10 秒获取一次快照，也可手动刷新。命令行可读取 `GET /api/v1/health`。

两个 Rust 服务将英文日志输出到 stderr，`logging.level` 控制级别。可以通过提交 ID 关联后端分派、结果保存和 Worker 执行日志。

停止时先向后端发送 Ctrl-C 或 SIGTERM，后端在消息预算内收尾已登记的本地操作和当前消息，不等待远程判题；Worker 按沙箱及消息预算停止。后端重启只重交 queued，恢复 dispatching/running 等待，旧 PENDING 审查标记中断。消费者在阻塞线程通过 RAII 析构，应用等待超时后底层析构仍可能继续；已保存的终态继续可查。

前端部署时构建 `web/dist/`，由静态服务器提供页面并将同源 `/api/` 代理到后端。远程浏览器使用 HTTPS，以支持创建提交所需的 `crypto.randomUUID()`。部署示例见[前端说明](web/README.md#构建与静态部署)。

## 开发检查

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p acorn-backend --locked --lib --test config_results
```

workspace 要求自写公共和私有 Rust 项提供 Rustdoc 文档，各 crate 继承统一 lint 配置。模块测试、协议生成和前端检查命令分别见对应 README；架构和接口细节见[文档索引](docs/README.md)。

数据库和 workflow 检查需要 `MYSQL_PASSWORD` 及[MySQL 测试配置](config/backend-test.example.toml)的私有副本，通过 `ACORN_TEST_DATABASE_CONFIG` 指定。开发账户为每个测试创建和删除独立数据库，见[后端检查说明](crates/backend/README.md#检查命令)。

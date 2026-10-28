# acorn-backend

acorn-backend 是 Acorn 的 HTTP 后端，使用 Axum、Tokio 和 SQLx。它管理题目查询、提交创建、Worker 调度、MySQL 持久化、健康观测及 LLM 代码审查。

## 功能与模块

| 模块                      | 职责                                                     |
| ------------------------- | -------------------------------------------------------- |
| `http`                    | JSON API、分页筛选、请求限制和错误响应                   |
| `problems`、`submissions` | 题目版本查询、幂等创建、提交详情和历史                   |
| `scheduler`               | 有界待发队列及许可、Kafka 任务发布、事件持久化和审查交接 |
| `database`                | MySQL 访问、短事务和条件状态更新                         |
| `llm_review`              | 自动及手动审查、独立队列、模型调用和最新意见保存         |
| `health`                  | 数据库与 Worker 共享采集、最近观测缓存                   |
| `config`、`logging`       | TOML 配置和英文 stderr 日志                              |

一个后端进程运行一个 Scheduler，可连接多个 Worker。每个提交固定创建时的题目版本，源码和测试数据随任务交给 Worker；提交 ID 同时作为 Worker Job ID。

库接口 `scheduler::kafka::Publisher` 和 `EventConsumer` 分别提供任务发布与事件消费，`EventProcessor` 共用状态持久化和审查交接。调用方先用 `Database::judging_recovery`／`Scheduler::restore` 恢复本地所有权，再运行两项服务，提供停止信号并监督错误。Kafka 组位点需由部署初始化，运行规则见 [Kafka 设计](../../docs/10-kafka-judging-design.md)。正式入口通过 Scheduler::start 恢复并监督两项 Kafka 服务。

## 启动

先准备可互通的 Kafka、按[部署合同](../../docs/10-kafka-judging-design.md#10-整体部署)创建 topic 并初始化两端消费组位点，再按 [初始化说明](../../docs/09-mysql-migration-design.md#7-sql-改写与全新初始化)初始化数据库并预置用户、题目，再按 [Worker 说明](../worker/README.md#启动)启动节点。在仓库根目录执行：

```sh
cargo build -p acorn-backend --locked
mkdir -p "$HOME/.config/acorn"
cp config/backend.example.toml "$HOME/.config/acorn/backend.toml"
```

编辑配置，核对 MySQL 端点、库名、账户、用户 ID 和 Worker 地址，然后启动：

```sh
read -r -s -p 'MySQL 密码：' MYSQL_PASSWORD
printf '\n'
export MYSQL_PASSWORD
target/debug/acorn-backend --config "$HOME/.config/acorn/backend.toml"
```

应用连接已初始化的 MySQL 8.4／InnoDB 数据库，只接受结构版本 4。启动检查版本及预置用户，缺少密码时明确失败。命令行也支持 `--help` 和 `--version`。

预置开发账号为 `test`，显示名 `Test User`，密码为 `Acorn-Test-Only-123!`，SQL 保存对应 Argon2id 哈希。后端库提供账号读写、输入校验及密码哈希／验证能力；当前 HTTP 尚无注册、登录或会话接口，提交归属仍由 `test_user.user_id` 指定。

## 配置

完整字段见 [backend.example.toml](../../config/backend.example.toml)。已有部署配置须删除 `scheduler.poll_interval_ms` 及 `grpc.submit_timeout_ms/get_timeout_ms/ack_timeout_ms`。

| 配置节       | 主要设置                                     | 示例值                                                            |
| ------------ | -------------------------------------------- | ----------------------------------------------------------------- |
| `http`       | 监听地址、JSON 请求上限、默认及最大页长      | `127.0.0.1:8080`，512 KiB，20 / 100                               |
| `database`   | 端点、库名、用户名、连接池上限及等待预算     | `127.0.0.1:3306`、`appdb`、4 个连接、5000 ms 取得等待、3 秒锁等待 |
| `test_user`  | 所有提交的归属用户 ID                        | `1`                                                               |
| `scheduler`  | 状态地址列表、待接纳容量、Kafka 发布重试间隔 | 128 个名额，1000 ms                                               |
| `grpc`       | 状态 RPC 的消息发送及接收上限                | 16 MiB / 1 MiB                                                    |
| `request`    | 源码、用例、原始载荷及单用例资源上限         | 源码 64 KiB，最多 128 个用例                                      |
| `results`    | 编译诊断、stdout、stderr 和故障说明保留上限  | 前三项各 64 KiB，故障说明 512 字节                                |
| `health`     | 每轮采集后的间隔、单项探测超时               | 5000 / 1000 ms                                                    |
| `llm_review` | 审查开关、模型地址、凭据、并发和预算         | 默认关闭；详见下文                                                |
| `logging`    | 日志过滤级别                                 | `info`                                                            |

数据库密码由进程环境变量 `MYSQL_PASSWORD` 提供，TOML 只保存非秘密连接字段；正式程序不自动加载 `.env`。MySQL SSL 显式禁用。API、Scheduler、审查与健康采集共用连接池，每条新连接设置 utf8mb4_0900_bin、READ COMMITTED、严格模式及两类锁等待。取得连接的等待预算与查询、锁等待分别计量；模型和 Worker 网络等待不借用数据库连接。

后端的任务、执行及结果预算须与所有 Worker 对齐。待接纳容量包含已预留、已排队和正在等待 Worker 接纳的提交；Worker 接纳后释放该名额。

HTTP 和 gRPC 的访问范围由部署端配置。后端使用配置中的预置用户作为提交归属；浏览器通过同源反向代理访问 API，静态页面由前端部署服务提供。

### Kafka 客户端配置

Kafka 4.3.1 单 broker 的启动、topic 及首次消费组位点初始化见[共享部署说明](../kafka/README.md#单-broker-部署)，两端启动前须完成该初始化。

构建默认动态链接系统 librdkafka（至少 2.12.1），需要开发包及 pkg-config；静态构建方法见[根说明](../../README_zh-CN.md#构建)。仅构建后端时可执行 `cargo build -p acorn-backend --locked --no-default-features --features kafka-static`。

`[kafka]` 必填，`bootstrap_servers`、`tasks_topic`、`events_topic` 和 `group_id` 必须显式配置。端点采用 `host:port` 或 `[IPv6]:port`，不带 URL 协议；两个 topic 必须不同。示例使用 `localhost:9092`，实际端点须与 broker 对客户端广播的可达地址一致。使用已有私有配置时，将该节合并到实际 `--config` 指定的文件，其余数据库和审查配置保留原值；数值预算可省略并使用下表默认值。

| 字段                     | 默认值     | 含义                         |
| ------------------------ | ---------- | ---------------------------- |
| `delivery_timeout_ms`    | `30000`    | 单次投递及客户端内部重试预算 |
| `queue_timeout_ms`       | `1000`     | 等待进入生产者队列的预算     |
| `shutdown_timeout_ms`    | `5000`     | 正常停机的总消息收尾预算     |
| `max_task_record_bytes`  | `16777216` | 编码任务上限，最多 16 MiB    |
| `max_event_record_bytes` | `1048576`  | 编码事件上限，最多 1 MiB     |

时间预算必须为正且不超过 `2147483647` ms。编码上限须容纳配置允许的原始任务和有界结果及 Protobuf 开销。

后端在必要数据库写入完成后，使用 SDK safe 异步接口发起事件位点提交；不读回、不逐条等待 broker 回执。请求入队失败仍先完成已保存审查的本地交接，再报告错误并结束消费服务。入队成功仅表示 SDK 已受理，位点可能落后并产生事件重放；已有终态和审查按幂等规则处理。既有 `[kafka]` 配置须删除 `commit_timeout_ms`；该字段已移除，保留它会按未知字段拒绝启动。

独立的 `[worker_membership]` 可省略，`tasks_group_id` 默认 `acorn-judge-workers-v1`，须与 Worker 的 `[kafka].group_id` 一致；`interval_ms` 默认 `5000`，`timeout_ms` 默认 `1000`；样本新鲜期限为 `2 × (interval_ms + timeout_ms)`，默认 12 秒。间隔必须为正，查询预算使用正的客户端毫秒范围，派生期限不可溢出。此配置与 `[health]` 独立。后台只查询该组，不加入它；新鲜零成员或 unknown 在新建入库前返回 `503 WorkersUnavailable`，已有同键提交沿原规则返回或重新交接。正成员数仍受 Scheduler 容量限制，不表示空闲执行位置。

两端配置直接使用 [acorn-kafka](../kafka/README.md) 的普通 `KafkaConfig`。`scheduler::kafka::producer_config(&config.kafka)`／`consumer_config(&config.kafka)` 分别选择任务生产／事件消费，并调用共享客户端策略：PLAINTEXT、禁止自动建 topic、幂等生产及 `acks=all`、手动位点和 `auto.offset.reset=error`、classic 消费组。任务生产者和事件消费者的传输上限分别在编码上限外预留 1024 字节，接收上限再预留 512 字节；生产队列最多 128 条且字节上限为两个最大记录，预取队列字节软上限为一个最大记录，可被单次 fetch 超过。

既有后端配置的 `[kafka].events_group_id` 改名为 `group_id`，保留原事件消费组值。

正式入口使用上述 Kafka 客户端发布任务并消费事件，gRPC 仅查询状态。消息接口见[共享协议](../protocol/README.md#kafka-消息编解码)。

## HTTP API

| 方法 | 路径                                          | 用途                     |
| ---- | --------------------------------------------- | ------------------------ |
| GET  | `/api/v1/problems`                            | 已发布题目列表           |
| GET  | `/api/v1/problems/{id}`                       | 最新题面、语言及资源限额 |
| GET  | `/api/v1/problems/{id}/versions/{version_no}` | 指定已发布版本           |
| POST | `/api/v1/submissions`                         | 创建提交                 |
| GET  | `/api/v1/submissions`                         | 提交历史                 |
| GET  | `/api/v1/submissions/{id}`                    | 判题及审查快照           |
| POST | `/api/v1/submissions/{id}/review`             | 启动或重新审查           |
| GET  | `/api/v1/health`                              | 本地状态和最近组件观测   |

列表使用 `limit`、`offset` 分页，返回 `items`、`limit`、`offset`。提交历史支持 `problem_id`、`state`、`verdict` 和 `review_status` 筛选，多个条件取交集：

```sh
curl --fail-with-body 'http://127.0.0.1:8080/api/v1/problems?limit=20&offset=0'
curl --fail-with-body 'http://127.0.0.1:8080/api/v1/submissions?problem_id=1&state=finished&verdict=AC'
```

创建提交需要 JSON 字段 `problem_id`、`language_id` 和 `source`，以及 UUID 格式的 `Idempotency-Key` 请求头。同键同内容返回原提交；同键不同内容返回 `409 IdempotencyConflict`。客户端丢失响应时，可用原键与原内容重传。

### 从源码文件提交

在仓库根目录将源码文件编码为请求 JSON，再发送：

```sh
python3 - c examples/submissions/a-plus-b.c > /tmp/acorn-request.json <<'PY'
import json
from pathlib import Path
import sys

print(json.dumps({
    "problem_id": 1,
    "language_id": sys.argv[1],
    "source": Path(sys.argv[2]).read_text(encoding="utf-8"),
}))
PY
ACORN_REQUEST_KEY=$(python3 -c 'import uuid; print(uuid.uuid4())')
curl --fail-with-body -i http://127.0.0.1:8080/api/v1/submissions \
  -H 'Content-Type: application/json' \
  -H "Idempotency-Key: $ACORN_REQUEST_KEY" \
  --data-binary @/tmp/acorn-request.json
```

新提交返回 `202` 和 `Location`；重复接纳返回 `200`。按 `Location` 查询到终态。重传时只重复上述 curl，保留请求文件和原 `ACORN_REQUEST_KEY`；主动重新判题时生成新键。

| 源码样例                                                      | `language_id` | 预期结论 |
| ------------------------------------------------------------- | ------------- | -------- |
| [a-plus-b.c](../../examples/submissions/a-plus-b.c)           | `c`           | AC       |
| [a-plus-b.py](../../examples/submissions/a-plus-b.py)         | `python`      | AC       |
| [compile-error.c](../../examples/submissions/compile-error.c) | `c`           | CE       |
| [wrong-answer.py](../../examples/submissions/wrong-answer.py) | `python`      | WA       |
| [time-limit.c](../../examples/submissions/time-limit.c)       | `c`           | TLE      |

提交状态依次为 `queued`、`dispatching`、`running`，最终为 `finished` 或 `failed`。`finished` 携带 AC、WA、CE、RE、TLE、MLE、OLE 之一；`failed` 携带执行或消息协议错误。CE 返回编译诊断，用例失败返回首个失败用例的输出及可用资源统计。HTTP 中的字节字段使用 Base64；资源统计使用微秒和字节。

### 错误响应

错误响应形如 `{"error":{"code":"QueueFull","message":"The submission queue is full."}}`。使用 `error.code` 区分错误；`QueueFull` 表示待接纳容量已满，`BackendStopping` 表示服务正在停止接纳。完整字段和错误码见 [HTTP 契约](../../docs/03-backend-design.md#5-http-接口)。

## LLM 代码审查

审查检查源码中针对用例的硬编码、猜测和规避行为，保存摘要及包含行号、证据、可能正常解释的疑点。结果通过 `review_status` 和 `review` 返回，与机器判题结论分别展示。

在服务端 TOML 中配置完整 Responses POST URL、密钥和模型，重启后启用：

```toml
[llm_review]
enabled = true
endpoint = "https://api.deepseek.com/responses"
api_key = "<填写服务端密钥>"
model = "deepseek-flash"
reasoning_effort = "max"
timeout_ms = 120_000
max_concurrent = 2
pending_max_count = 16
max_output_tokens = 10_240
max_request_bytes = 1_048_576
max_response_bytes = 1_048_576
```

程序从 TOML 读取凭据。模型输入为保存的源码、语言、固定版本公开题目标题和题面，以及机器 verdict。审查使用独立队列；读取输入后归还数据库连接，模型等待期间 Worker 可继续处理其他提交。

自动策略来自题目版本的 `review_mode`：`DISABLED` 关闭，`ENABLE-ANY` 要求已执行用例中至少一例通过，`ENABLE-ALL` 要求 AC。手动审查适用于所有 `state=finished` 的提交，包括 CE 和首例失败。

### 手动启动与重跑

先 GET 最新提交详情。首次审查使用 `expected_review_id=null`；重新审查时使用最新 `review.review_id` 的 UUID 字符串：

```sh
ACORN_SUBMISSION_ID='<实际 submission_id>'
curl --fail-with-body "http://127.0.0.1:8080/api/v1/submissions/$ACORN_SUBMISSION_ID"
curl --fail-with-body -i \
  -X POST "http://127.0.0.1:8080/api/v1/submissions/$ACORN_SUBMISSION_ID/review" \
  -H 'Content-Type: application/json' \
  -d '{"expected_review_id":null}'
```

新审查返回 `202`；已有 PENDING 返回 `200`，复用同次审查。继续 GET 到 `PASS`、`DOUBTFUL` 或 `FAILED`。重新审查会用新 ID 和 PENDING 替换上次意见。收到 `ReviewChanged` 或 POST 结果不明时，先 GET 确认最新状态再决定是否发送请求。

审查状态筛选示例：

```sh
curl --fail-with-body 'http://127.0.0.1:8080/api/v1/submissions?review_status=DOUBTFUL&limit=20'
```

策略、输出和预算细节见 [LLM 审查设计](../../docs/08-llm-code-review-design.md)。

## 健康观测与停机

`GET /api/v1/health` 返回提交接纳条件、Scheduler 状态及名额、数据库读取、Kafka 两个 topic 的 metadata 和 Worker 最近探测结果。探测由后台共享采集执行，GET 读取缓存；默认每轮结束后等待 5 秒，单项超时 1 秒，观测达到 12 秒时标为 `stale`。

Ctrl-C 或 SIGTERM 停止新接纳、发布和审查，等待已登记创建交接后，在共同消息预算内收尾当前发送、事件持久化和消费者 RAII 释放，再关闭 HTTP、健康采集和数据库；独立成员观测在停止接纳后结束。停机期间查询继续可用，不等待远程 Worker 判题。重启只重交 queued，恢复 dispatching/running 等待；旧 PENDING 审查保存为 FAILED/ReviewInterrupted。SDK 析构在阻塞线程执行，超时不会取消底层析构，进程退出仍可能等待它。

日志输出到 stderr，以提交 ID 关联创建、分派和终态保存事件。关键数据库写入或调度任务失败会触发进程退出，错误原因写入日志。

## 检查命令

Kafka 消息往返检查使用可创建／删除测试 topic 的独立测试环境，创建 UUID 命名的测试 topic，检查大任务及运行／终态事件的原始字节，结束后删除这些 topic：

```sh
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-backend --locked --test kafka -- --ignored --nocapture
```

该检查采用测试 topic 的显式起始位点，不使用业务组或改变业务位点。Kafka 服务与客户端须处于可互通的网络环境。

在仓库根目录执行：

```sh
cargo test -p acorn-backend --locked --lib --test config_results
ACORN_TEST_DATABASE_CONFIG=/path/to/mysql-test.toml \
  cargo test -p acorn-backend --locked --test database --test workflow -- --test-threads=1
```

数据库和 workflow 检查同时需要进程 `MYSQL_PASSWORD`；测试 TOML 采用 [backend-test.example.toml](../../config/backend-test.example.toml) 的字段，其中 name 是 1–32 位 ASCII 字母数字库名前缀，每例追加随机 UUID。测试账户需具备该前缀范围内的建库、DDL／DML 及删库权限，拒写和表锁检查另需 TRIGGER／LOCK TABLES。夹具通过三个正式 SQL 初始化独立库，正常收尾先停止服务、关闭连接池再删除本例库；失败残留由开发端处置。测试不自动加载 `.env`，测试账户权限与正式应用账户分开配置。

启用 binlog 时，服务器还须允许该账户创建测试触发器，否则可能返回 1419。由管理员预先准备开发实例，所需设置及影响范围见[迁移测试环境说明](../../docs/09-mysql-migration-design.md#112-命令与环境)；测试本身不修改服务器全局变量。

使用已配置 Kafka 和沙箱的主机检查真实 Worker 链路，选择空闲的专用 box ID：

```sh
ACORN_TEST_KAFKA_SERVERS=localhost:9092 ACORN_TEST_DATABASE_CONFIG=/path/to/mysql-test.toml ACORN_ISOLATE_BOX_ID=204 \
  cargo test -p acorn-backend --locked --test workflow real_http_worker_acceptance -- --ignored --nocapture
```

双 Worker 并行检查还需第二个空闲的专用 box ID，两者不能相同：

```sh
ACORN_TEST_KAFKA_SERVERS=localhost:9092 ACORN_TEST_DATABASE_CONFIG=/path/to/mysql-test.toml \
  ACORN_ISOLATE_BOX_ID=204 ACORN_ISOLATE_BOX_ID_2=210 \
  cargo test -p acorn-backend --locked --test workflow real_two_workers_overlap -- --ignored --nocapture
```

该用例将两份不同提交投递至不同 Worker 的任务分区，直接通过状态 RPC 确认执行重叠，再核对两份持久化 AC；HTTP 创建及发布由前一项主要判题路线检查覆盖。

后端进程的 Kafka 恢复、错误监督及审查停机检查需要先构建同一工作区的 Worker。两端二进制闭环使用现有受控 Isolate 管理器，只验证编排：

```sh
cargo build --workspace --locked
ACORN_TEST_WORKER_EXECUTABLE="$PWD/target/debug/acorn-worker" \
  ACORN_TEST_KAFKA_SERVERS=localhost:9092 ACORN_TEST_DATABASE_CONFIG=/path/to/mysql-test.toml \
  cargo test -p acorn-backend --locked --test workflow process_ -- --ignored --test-threads=1
```

workspace 格式、编译及 Clippy 命令见[根说明](../../README_zh-CN.md#开发检查)，组件职责与调度合同见[后端设计](../../docs/03-backend-design.md)。

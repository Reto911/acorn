# 后端与 Scheduler 设计

状态：已确认，作为首版实现依据。

实施进度：P06/P07 已完成配置、日志、SQLite 业务访问、开发用户预置及 Scheduler 库模块；HTTP 与程序入口组装按 P08 实施，启动恢复与完整故障边界仍按 P09 实施。

## 1. 目标与范围

后端是单个 Rust 进程，内含 HTTP API、Scheduler 和数据库访问模块。前端通过 HTTP JSON 查询题目、提交单文件源码并轮询结果；Scheduler 通过 unary gRPC 向独立 Worker 推送任务、轮询状态和确认结果。

首版服务于 C 与 Python 3 判题闭环。题目、测试数据及一个测试用户由部署端本地预置；后端从配置读取该用户的 `user_id`，所有新提交归属于此用户。当前不启用登录、会话、角色鉴权或用户管理接口，请求不能覆盖提交归属。

前后端完全分离，后端仅提供 API，不托管前端静态文件，也不实现 CORS 中间件。HTTP 访问边界和浏览器跨域响应由部署端通过防火墙、反向代理及路由配置安排。

执行规则以 [Worker 设计](01-worker-design.md)为准，存储结构以[数据库设计](02-database-design.md)和[建表 SQL](../sql/001-initial-schema.sql)为准。Scheduler 只有一个活动实例，与 API 一同启动和退出。

## 2. 技术选型与进程结构

| 用途 | 技术 |
| --- | --- |
| 实现语言 | Rust |
| HTTP | [Axum](https://docs.rs/axum/latest/axum/)，请求和响应使用 Serde / serde_json |
| 异步执行 | 一个 Tokio runtime，运行 HTTP、Scheduler、gRPC 和数据库异步调用 |
| Worker 客户端 | [Tonic](https://docs.rs/tonic/latest/tonic/) / Prost，依据 `acorn.worker.v1` 协议生成 |
| HTTP 中间件 | Tower 的 `ServiceBuilder` 组合中间件，tower-http 的 `TraceLayer` 记录请求日志 |
| 数据库 | SQLite，使用 [SQLx](https://docs.rs/sqlx/latest/sqlx/) 的 `runtime-tokio`、`sqlite` 特性，直接维护 SQL |
| 配置 | TOML，启动时读取并验证 |
| 日志 | tracing / tracing-subscriber，英文单行文本输出到 stderr |

具体依赖版本在实施时统一选择和锁定。随 SQLite 驱动构建的 SQLite 必须满足初始 SQL 对 STRICT 表和 JSON 函数的要求。

```mermaid
flowchart LR
    Frontend[前端] -->|HTTP JSON| HTTP
    subgraph Backend[后端进程]
        HTTP[HTTP API] --> Service[题目与提交服务]
        Service -->|完整任务数据| Scheduler
        Service --> Database[数据库访问]
        Scheduler --> Database
        subgraph Scheduling[Scheduler]
            Scheduler[队列与调度] --> Client[worker_client]
        end
    end
    Database --> SQLite[(acorn.db)]
    Client -->|提交、轮询、ACK| Worker[独立 Worker 进程]
```

API 与 Scheduler 通过进程内调用和消息交接任务。Worker 客户端复用配置地址对应的 gRPC channel；连接复用不改变短 RPC、轮询取结果的业务模型。

## 3. 模块职责

模块先按后端程序内的 Rust module 组织，共同拥有一个进程生命周期。

| 模块 | 职责 |
| --- | --- |
| `http` | 路由、HTTP 请求与响应结构、请求大小限制、状态码和错误响应转换 |
| `problems` | 查询已发布题目的最新版本、题面和语言限额 |
| `submissions` | 创建幂等、固定版本、校验请求、保存源码、将任务交给 Scheduler、查询历史与结果 |
| `scheduler` | 有界内存队列、Worker 轮转选择、分派、轮询、终态保存和 ACK 编排 |
| `scheduler::worker_client` | Scheduler 子模块，封装三个 Worker RPC、调用 deadline、Proto 与后端类型转换、传输错误分类 |
| `database` | SQLx 共享连接池、参数绑定、短事务、条件状态更新和查询 |
| `config` / `logging` | 启动配置校验和统一日志初始化 |
| 程序入口 | 组装共享句柄、完成启动检查、监督后台任务、处理退出 |

HTTP 请求与响应结构也称 DTO（Data Transfer Object，数据传输对象），例如创建请求的 `problem_id`、`language_id`、`source`，以及创建响应的 `submission_id`、`state`。它们定义对外 JSON 形状，在边界与 Worker Proto、数据库行分别转换。后端使用少量明确的数据类型表示任务和结果。

HTTP 层通过 `ServiceBuilder` 组合公共中间件，`TraceLayer` 统一记录方法、路径、状态码和耗时；请求体上限使用 Axum 的 `DefaultBodyLimit`。JSON 提取、路径与查询参数解析和路由拒绝均转换为第 5.4 节的错误结构。Worker 选择、任务去重和重新排队由 Scheduler 控制，gRPC deadline 由客户端设置。

完整任务包含 Submission ID、固定题目版本、语言、源码字节、有序用例和限额。源码从 HTTP 字符串按 UTF-8 编码，不调整换行、不裁剪内容；入库字节与发送给 Worker 的字节一致。

HTTP 查询读取数据库中的业务状态和结果。前端轮询不触发 Worker RPC；Worker 的阶段和逐点进度不写库，首版 HTTP 也不提供这些临时信息。

## 4. 数据库访问与预置数据

API 与 Scheduler 共享一个 SQLx `SqlitePool`，首版固定 `max_connections = 1`，数据库访问依次使用该连接。SQLx 异步调用直接运行在后端已有的 Tokio runtime 内；SQLite 的阻塞调用由 SQLx 内部后台线程处理，见[运行时支持](https://docs.rs/sqlx/latest/sqlx/#runtime-support)和[SQLite 连接说明](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnection.html)。

一个数据库操作使用一个连接，完成一次查询或一个短事务后归还；占用连接和事务均不能跨越 gRPC 调用或等待 Scheduler 入队。创建提交仍使用 `BEGIN IMMEDIATE`，表结构和事务规则保持不变。连接显式启用 `foreign_keys` 并配置有限 busy timeout。使用参数绑定的普通 SQL 查询和类型映射。

服务使用已初始化的 `acorn.db`，启动时检查结构版本及配置引用，不自动修改已有结构或执行升级。

部署预置的测试用户必须存在，题目按数据库设计在事务内发布，包含至少一个用例和允许语言的正值预算。测试用户的密码哈希字段保留，当前后端不执行密码验证或生成登录凭据。

后端仍验证语言、用例数量、数据大小和限额：源码按 UTF-8 编码后的长度计量；输入、期望输出及总载荷按原始字节计量。HTTP 请求体、gRPC 消息和原始任务载荷各有独立上限。

内存限额从 SQLite `memory_kib`、配置 `request.max_run_memory_kib` 到 RPC `RunLimits.memory_kib` 统一使用 KiB，不进行字节除法。数据库结构版本为 2，旧版数据库明确拒绝；后端与 Worker 需使用同一版 KiB 协议。实际资源统计和结果字节字段的单位保持原契约。

同一版本的测试数据在一次任务装配中按 `case_index` 读取。正常新提交由 API 将内存中的完整任务直接交给 Scheduler，Scheduler 不再次读源码或题库。启动装入排队任务时才由 Scheduler 读取其固定版本数据。

P06 的 [`Database`](../crates/backend/src/database/mod.rs) 提供题目列表与版本查询、创建幂等查询、提交详情与筛选历史、完整固定版本任务装配，以及三个非终态条件更新、终态保存和旧在途中断写入。`create_submission` 在短事务内查重、固定版本并装配 `JudgingTask`，提交并释放连接后返回；待接纳名额由后续调用服务预留，数据库层不等待队列。已有同键同内容记录保留原版本；仅 queued 记录返回可交接任务。`queued_tasks` 按顺序读取，并在超过配置容量时明确报错；入口尚不调用这些启动状态处理，统一编排在 P09 接入。

终态先由 [`TerminalOutcome`](../crates/backend/src/submissions.rs) 按已知用例数和原始字节预算校验结果形状，再执行单条条件 UPDATE，同时原子提交状态、独立 verdict、JSON 详情和终态时间。P07 的 Worker 客户端校验完整响应的 Job ID、运行状态、阶段及进度与终态消息对应关系。数据库错误保留 SQLx 原始原因，业务查重与校验错误使用独立类型；Scheduler 关键写入错误使调度任务返回错误，入口监督及完整故障退出边界在 P08/P09 接入。

配置示例见 [backend.example.toml](../config/backend.example.toml)。`scheduler.workers` 使用 Tonic endpoint URI；数据库路径相对于进程工作目录。HTTP 体积按源码最坏 JSON 转义开销预留，gRPC 门限按原始任务／结果另加协议空间校验，Base64 JSON 长度不复用二进制消息门限。P06 程序入口完成配置、结构版本和预置用户检查后关闭连接，明确报告服务尚未实现并返回 1。

## 5. HTTP 接口

接口使用 HTTP JSON、HTTP 头中的幂等键及 UTF-8 源码。

### 5.1 路由与查询

| 方法与路径 | 行为 |
| --- | --- |
| `GET /api/v1/problems` | 返回已发布题目列表，只展示最新版本 |
| `GET /api/v1/problems/{id}` | 返回最新版本的题号、版本号、标题、题面和可提交语言及限额 |
| `GET /api/v1/problems/{id}/versions/{version_no}` | 返回指定已发布版本的同类信息；前端不默认接入 |
| `POST /api/v1/submissions` | 创建提交，或取得同键的已有提交 |
| `GET /api/v1/submissions/{id}` | 返回提交身份、固定版本、语言、业务状态、判题结论、终态详情与时间戳 |
| `GET /api/v1/submissions` | 返回配置测试用户的提交历史，可按题号、状态和 verdict 筛选 |

列表使用 `limit`、`offset`，默认页长和最大页长由配置提供。题目按题号排序，历史按 `created_at_ms DESC, id DESC` 排序；列表不附带源码、题面正文或终态诊断。题目查询不返回测试输入和答案。

题目列表和默认详情使用最新版本；显式版本接口可查询仍保留的已发布版本，题号与版本不匹配或不存在时返回 `404`。它不改变创建提交的版本选择，新提交仍只接受题号并选取最新版本。提交中的版本号来自创建时的固定记录，结果展示和失败用例定位不得改用题目的最新版本。

### 5.2 创建提交

```http
POST /api/v1/submissions
Content-Type: application/json
Idempotency-Key: 7b5f28d3-51a4-463c-98a4-0a11815d8d46
```

```json
{
  "problem_id": 1,
  "language_id": "c",
  "source": "int main(void) { return 0; }\n"
}
```

`Idempotency-Key` 必填，格式为随机 UUID。请求只接受上述业务字段；用户、题目版本、测试数据、限额和执行参数由服务端确定。

新提交返回 `202 Accepted`，同键同内容返回 `200 OK`，均包含 `submission_id`、`state`，并通过 `Location` 指向提交查询地址。响应只确认业务提交已保存并交给本地队列，不等待 Worker 空闲或判题结束。

同键内容比较使用题号、语言及 UTF-8 编码后的源码字节；冲突返回 `409 IdempotencyConflict`。重复请求不重新选版本、不创建新 ID、不重新执行已经分派或结束的任务。客户端断开或丢失响应不撤销提交。

### 5.3 提交详情与判题结果

提交查询使用数据库的 `queued`、`dispatching`、`running`、`finished`、`failed` 状态，`verdict` 独立返回。终态详情作为类型化对象返回，字节字段使用 Base64；不向前端返回双重编码的 JSON 字符串。

`GET /api/v1/submissions/{id}` 返回以下字段。这些是 HTTP 响应字段：`submission_id` 对应数据库的 `submissions.id`，`terminal_details` 对应解析后的 `terminal_details_json`，不新增同名数据库列。

| 字段 | 内容 |
| --- | --- |
| `submission_id` | UUID 字符串 |
| `problem_id`、`version_no`、`language_id` | 创建时固定的执行对象 |
| `state` | 持久化业务状态 |
| `verdict` | 判题结论，未取得判题结论时为 `null` |
| `terminal_details` | 终态详情对象；运行中和 AC 时为 `null` |
| `created_at_ms`、`terminal_at_ms` | UTC Unix 毫秒；终态时间尚无时为 `null` |

运行中 `verdict` 和终态详情为空；AC 仅有结论；CE 带编译诊断和截断标记；WA、RE、TLE、MLE、OLE 只带首个失败用例；执行失败带 `code` 和有界 `diagnostic`。缺失的用例和统计保持缺失，不构造占位数据。

CE 的 `terminal_details` 使用 `compile_diagnostic_base64`、`compile_diagnostic_truncated`。用例失败使用 `failed_case` 对象，含 `case_index`、`stdout_base64`、`stderr_base64`、两个截断标记，以及可选的 `resources`、`exit_code` 或 `signal`。资源字段保持 Worker 的微秒和字节单位；未提供的字段省略，退出码和信号至多存在一种。

查询到判题失败仍返回 `200 OK`，由业务状态和结果描述失败。执行失败详情中的 `code` 属于任务结果，不是 HTTP 错误码。

### 5.4 HTTP 错误规约

错误结构采用 `{"error":{"code":"QueueFull","message":"The submission queue is full."}}`。`code` 稳定供前端判断，`message` 为英文，具体措辞可以改进。HTTP 错误只描述当前 HTTP 操作未能完成。

首版所有 HTTP `error.code` 如下，增加错误码时须先更新此表：

| `error.code` | HTTP 状态码 | 条件 |
| --- | --- | --- |
| `InvalidRequest` | `400` | JSON、路径或查询参数格式错误，必填值缺失、源码为空、出现未允许字段或分页与筛选值无效 |
| `InvalidIdempotencyKey` | `400` | 创建请求缺失 `Idempotency-Key`，或键不符合 UUID 格式 |
| `UnsupportedMediaType` | `415` | 创建请求未使用 JSON Content-Type |
| `RequestTooLarge` | `413` | HTTP 请求体超过上限 |
| `SourceTooLarge` | `413` | 源码按 UTF-8 编码后超过字节上限 |
| `UnsupportedLanguage` | `422` | 请求的语言不是首版支持的 C 或 Python |
| `LanguageNotAllowed` | `422` | 题目最新版本未允许请求语言 |
| `ProblemNotFound` | `404` | 题目不存在或尚无已发布版本 |
| `ProblemVersionNotFound` | `404` | 指定题目下不存在该已发布版本 |
| `SubmissionNotFound` | `404` | 提交不存在 |
| `IdempotencyConflict` | `409` | 同一用户的同一幂等键携带不同题目、语言或源码 |
| `QueueFull` | `503` | 当前没有待接纳名额 |
| `BackendStopping` | `503` | 后端已停止接纳新提交 |
| `RouteNotFound` | `404` | API 路径不存在 |
| `MethodNotAllowed` | `405` | 路径存在但不支持该 HTTP 方法 |
| `InternalError` | `500` | 数据库访问、预置任务数据或当前操作发生内部错误 |

已有排队提交因没有名额而无法再次交接时，`QueueFull` 的 `error` 对象额外携带 `submission_id`，指向原提交；其他错误省略该字段。内部 SQL、宿主路径和原始传输异常只写日志，不直接用作 `error.code` 或 `message`。框架及中间件的错误响应也遵守本节。

## 6. 提交创建与进程内交接

1. 校验 HTTP 请求、幂等键和源码大小，用配置中的测试用户确定归属。
2. 先查 `(user_id, create_request_key)`。已有记录按既定内容比较规则返回；只有仍为 `queued` 且未在内存中的记录需要重新交接。
3. 新建前原子预留一个待接纳名额，满时返回 `503`，不创建 Submission。预留只在内存中记录，不是数据库记录或 Worker 执行资源。
4. 在 `BEGIN IMMEDIATE` 短事务中再次查重，处理同时到来的同键请求。确实为新提交时，选取最新版本，装配并验证测试数据和限额，生成 UUID，插入 `queued` 记录和源码；竞争中发现已有记录时返回原记录并释放多余预留。
5. 数据库提交成功后，用已预留的名额将完整任务交给 Scheduler，再完成创建响应。回滚或校验失败释放预留。

创建服务拥有事务与入队这一短操作的生命周期，HTTP 处理器只等待它的结果。客户端断开后，已经开始的创建操作仍完成事务处理和提交后的入队；响应无人接收不撤销已保存的 Submission。正常停机等待这些操作结束，不把交接是否完成绑定到 HTTP 连接存续时间。

进程可能在数据库提交后、入队前崩溃；该提交仍为 `queued`，下次启动装入队列。数据库与内存交接不具有跨介质原子性，首版以启动装入和同键重传覆盖这一边界。

重复投递按 Submission ID 合并。终态记录的重复创建响应不占排队名额，即使队列已满也可以返回原提交。已有 `queued` 记录因队列满而无法再次交接时，返回带原 Submission ID 的 `503`，保留该排队记录。

## 7. Scheduler

### 7.1 内存状态与分派策略

P07 的实现位于 [`scheduler`](../crates/backend/src/scheduler/mod.rs)，短同步锁保护 FIFO 与按 ID 去重的本地记录；Worker 缓存由单个调度循环持有，各任务的 RPC 跟踪在独立 Tokio 任务中执行并回送完成事件，见 [`tracking`](../crates/backend/src/scheduler/tracking.rs)。数据库访问和网络等待均在锁外执行。

Scheduler 保存 FIFO 待接纳队列、按 Submission ID 去重的记录、当前提交到 Worker 地址的映射，以及各 Worker 的可用状态缓存。Worker 地址来自配置，上述调度状态不落库。

队列上限包含创建中的预留、排队任务和等待 `SubmitJob` 接纳确认的任务。Worker 明确接纳后归还该名额；明确忙而退回队列时继续占用原名额，避免重入队时挤占已经释放的空间。

实现使用 `Arc<tokio::sync::Semaphore>`，初始许可数等于待接纳上限。创建前通过 `try_acquire_owned()` 取得许可，无许可时立即返回 QueueFull；`OwnedSemaphorePermit` 随创建操作、排队任务和等待接纳的任务转移。明确接纳、本地失败、事务回滚或重复投递合并时释放相应许可，忙时重新排队继续持有。信号量作为唯一名额计数，启动装入的每条 queued 记录也持有一个许可，见 [Tokio 信号量](https://docs.rs/tokio/latest/tokio/sync/struct.Semaphore.html)。

每个 Worker 缓存以下本地状态：

| 状态 | 含义与变化 |
| --- | --- |
| 可尝试 | 当前无本地在途分派，且再次尝试时间已到；启动时各地址为此状态 |
| 本地占用 | 已为一个任务选定该地址，缓存其 Job ID；本次分派或跟踪结束后清除 |
| 暂缓尝试 | 收到忙、结果区满、停机拒绝，或提交、查询通信失败后，缓存下一次允许尝试时间；配置间隔到达后转为可尝试 |

在可尝试的 Worker 中按轮转顺序选择，选中后先标记本地占用。不同 Submission 可在不同 Worker 同时执行；同一 Worker 只有一个本地在途分派，同一 Job 的阶段和用例始终由 Worker 串行执行。

明确因忙、结果区满或停机而未接纳的任务退回队首，目标 Worker 转为暂缓尝试。重新入队后立即触发选择：有其他可尝试 Worker 就直接分派，不等待刚刚拒绝的节点。新任务入队、在途完成和再次尝试时间到达也触发选择；这些操作只检查内存，不轮询数据库任务队列。

缓存根据现有提交、查询和 ACK 调用结果更新，不增加状态探测 RPC。终态已保存且一次 ACK 调用已结束时，节点重新可尝试；明确非法请求保存失败后也释放本地占用。提交或查询失败而结束本地跟踪时，节点进入暂缓尝试。这一间隔只约束该地址接收后续新任务，不延后当前失败终态，也不重新执行旧任务。

可尝试表示可以向该节点发起提交，不能证明远端确实空闲；旧任务可能还在执行，结果区也可能已满。最终以 `SubmitJob` 的接纳或拒绝为准，缓存不提供实时状态保证。

### 7.2 分派与轮询

| 事件 | 持久化与动作 |
| --- | --- |
| 选择任务和 Worker | 条件更新 `queued → dispatching`，提交事务后记录内存路由，再调用 `SubmitJob` |
| 条件更新影响 0 行 | 丢弃过期内存任务，归还待接纳名额和本地占用，不发送 RPC |
| 接纳成功或已存在同 ID | 条件更新 `dispatching → running`，对同地址、同 ID 调用 `GetJob` |
| 明确忙、结果区满或停机且未接纳 | 条件更新回 `queued`，清除本次路由，将任务放回队首，暂缓该节点并立即尝试其他候选 Worker |
| 明确非法请求 | 保存 `failed`，记录 Worker 拒绝原因，归还待接纳名额并释放本地占用 |
| 提交超时或断连，接纳情况不明 | 只做一次有界 `GetJob`；有记录则继续跟踪，终态则保存；不存在或无法确认则保存失败 |
| 正常轮询失败或记录不存在 | 立即保存失败，结束本次跟踪，不向其他 Worker 重交该任务 |
| `GetJob` 返回运行状态 | 按配置间隔继续查询，阶段和进度不落库 |
| `GetJob` 返回终态 | 校验结果，持久化终态，提交成功后调用 ACK |

gRPC deadline 覆盖单次提交、查询或 ACK；不启用应用层自动重发提交或对冲调用。只对明确未接纳的任务重新分派；状态不明或已执行任务不自动重试。

首版一个 Submission ID 就是一个 Job ID。`SubmitJob` 返回重复记录时，继续查询该记录，不生成第二个 ID。所有 RPC 响应须与所查询 ID 对应。

后端保存的通信与协议失败代码为 `WorkerUnavailable`（通信故障或无法确认）、`WorkerJobNotFound`（查询明确无记录）和 `InvalidWorkerResponse`（身份、状态、阶段、进度或终态形状违反协议）。明确非法请求保存 Worker 的 `InvalidJob`／`UnsupportedLanguage` 原因；Worker 自身返回的合法失败终态保留其代码。诊断按配置限制 UTF-8 字节。无效响应保存失败，不视为已经通过校验的远端终态，不发送 ACK。

### 7.3 保存结果与 ACK

在后端校验 Worker 终态的基本协议形状：状态与消息对应、结论有效、AC/CE 没有失败用例、用例级错误有且仅有首个失败用例且序号在本任务范围内、输出不超限。未知统计保持缺失，不重新运行程序或重新比较输出。

`finished` 与 `failed` 的写入使用 `WHERE state IN ('dispatching','running')`，在同一事务中保存状态、独立 verdict、详情和终态时间。终态时间是后端保存时刻。

事务提交后调用 `AcknowledgeResult`。ACK 失败仅记录警告，不回滚数据库、不重判；首版不建立自动 ACK 重试循环或持久确认队列。Worker 已返回终态且本次 ACK 调用已结束后，释放本地在途记录和路由。

迟到结果不能覆盖已保存失败；如果在本次仍有路由的处理流程中拿到旧终态，可丢弃内容并 ACK。任务跟踪结束后，不额外扫描旧 Worker 查找迟到结果。

## 8. 启动、停机与故障

### 8.1 启动

1. 加载并校验 TOML，初始化日志；检查至少一个且不重复的 Worker 地址、正值门限和配置测试用户。
2. 打开已初始化的数据库，检查结构版本，启用外键和数据库连接配置。
3. 将 `dispatching` 和 `running` 记录保存为 `failed / SchedulerInterrupted`，已有终态不变。
4. 检查现有 `queued` 数量不超过待接纳上限，按 `created_at_ms, id` 装入完整任务；超限报错退出，由部署端调整上限。
5. 创建 Worker 客户端句柄，启动 Scheduler 和 HTTP 服务；全部启动检查成功后才接纳 HTTP 提交。

排队任务继续调度；在途旧任务不恢复、不重交、不补发 ACK。没有 Worker 可立即接纳时，排队任务留在内存等待候选地址下次可用。

### 8.2 正常停机

收到退出信号后，停止接纳新提交和启动新分派。已经开始的创建流程完成数据库与本地交接；尚未分派的任务保留 `queued`，下次启动再装入。

等待已经分派的任务完成正常跟踪、结果保存和一次有 deadline 的 ACK 调用，再关闭 HTTP、SQLx 连接池和日志。明确未接纳而退回队列的任务不再分派。停止后端不向 Worker 发送远程取消请求。

被强制终止时，下次启动仍按第 8.1 节处理，不承诺这一次等待完成。

### 8.3 数据库及后台故障

连接配置有限 `busy_timeout_ms`，对 SQLite 支持的锁竞争先有限等待。关键写入经过等待仍无法完成时，记录英文错误，停止接纳与分派并退出后端；未成功保存的结果不 ACK。数据库不可写时，也不能声称已将任务保存为失败。部署修复并重启后按正常启动规则处理。

磁盘满、I/O 故障和损坏等错误直接进入上述退出路径，不进入通用自动重试或修复循环。预期的查重与业务校验错误正常返回，不触发进程退出。

有限等待是 SQLite 的连接机制，不是重新执行判题。其行为及与其他错误的区别见 [busy timeout](https://www.sqlite.org/c3ref/busy_timeout.html)与[结果码说明](https://www.sqlite.org/rescode.html)。

Scheduler 后台任务意外退出交给程序入口记录错误并结束进程，避免 HTTP 继续接纳而没有实际调度。SQLx 操作错误按上述数据库规则处理。此策略定义首版错误边界，后续改变恢复要求时单独设计。

## 9. 配置与日志

后端启动时读取一份 TOML，修改后通过重启生效。配置至少包含：

| 类别 | 配置内容 |
| --- | --- |
| HTTP | 监听地址、请求体上限、列表默认及最大页长 |
| 数据库 | `acorn.db` 路径、有限 `busy_timeout_ms` |
| 测试用户 | 已预置的 `user_id` |
| Scheduler | 待接纳上限、Worker 地址列表、轮询间隔、拒绝后再次尝试间隔 |
| gRPC | 提交、查询和 ACK deadline，发送与接收消息上限 |
| 任务校验 | 源码、用例数、单个输入和答案、合计字节、运行时间及内存的允许上限 |
| 结果与日志 | 有界诊断及输出约定、日志级别 |

API 上限与部署的 Worker 规约对齐，Base64 结果和 JSON 请求开销单独计量。配置不从 Worker 动态发现，不协商能力，不自动放宽限额。

日志统一为英文、可读的单行文本，记录提交 ID、题号、语言、Worker 地址、操作和错误原因。创建、分派、终态保存和停机记录为 info；明确拒绝和 ACK 失败为 warn；数据库与后台故障为 error；重复请求和普通轮询为 debug。日志不输出完整源码、测试数据或无界程序输出。

## 10. 最小验证范围

测试覆盖以下主要且可重复的流程；失败路径用可控的返回值验证既定行为，不刻意构造小概率、难以复现的时序窗口或故障组合。只为实际发现的问题补充回归测试。

- C 和 Python 各完成一次从 HTTP 创建、进程内排队、gRPC 执行到结果查询的真实闭环；AC、CE 和首个失败用例的返回遵守既有契约。
- 同键重传只有一个 Submission，冲突明确报错，题目更新后原提交仍固定原版本。
- 默认题目查询返回最新版本，显式版本接口可查旧版本；新提交仍不接受指定旧版本。
- 队列满时新请求不创建记录；重复已知提交能返回；明确忙后任务退回队首，有其他候选 Worker 时立即分派；跨 Worker 同时执行不同提交不改变单 Job 串行规则。
- 提交响应不明只查询一次；失联后保存失败且不重交；数据库终态提交早于 ACK，ACK 失败不改变终态。
- 后端重启继续装入 `queued`，将旧在途记录保存失败，不联系旧 Worker；启动排队超限和关键数据库故障按约定退出。
- 正常停机完成在途处理，排队记录留至下次启动。

# 健康观测设计与实施计划

版本：v2。本文定义 F08 的观测来源、状态 RPC、共享采集、HTTP、页面刷新、过期语义、任务划分及验收要求。范围确认见[审核记录](d01-review/07-health-observation-review.md)，实际过程与检查见[实施记录](d02-execution/06-frontend-development-execution.md#58-f08h01worker-状态-rpc)。

## 1. 目标与范围

为浏览器提供 HTTP 响应、数据库读取、Kafka topic 元数据、本地新提交接纳、Scheduler 生命周期和 Worker 状态的只读观测。依据见[前端健康规划](05-frontend-design.md#62-后续健康功能)和[F08](06-frontend-development-plan.md#38-f08后续健康观测)。

健康观测依赖 P01–P10 的后端和 F01–F07 的 GUI，作为独立任务划分。后端程序入口启动并监督唯一共享采集任务，正常和故障退出均在关闭数据库前停止并等待采集任务。

本版交付一个 HTTP GET 接口、一个 Worker 状态 RPC、一个后端共享探测任务和一个手动和自动刷新的页面。复用现有 Tokio、Tonic、SQLx、Axum 和前端查询组件，无需新增依赖、数据库表或独立服务。

健康观测结果不参与 Worker 选择、提交接纳、Job 状态转换或消息交接。独立的 Worker 消费组成员观测只约束新建接纳，遵守 [Kafka 成员观测合同](10-kafka-judging-design.md#56-worker-在线观测与接纳)，不并入健康缓存或本地接纳摘要。原有[调度和故障规则](03-backend-design.md#7-scheduler)继续生效。本版包含页面内的观测过期警告；外部告警、自动修复、服务重启、历史监控数据、资源监控和定时试判不在范围内。

## 2. 状态的含义与保证范围

| 对象       | 观测来源                                 | 能说明什么                                             | 保证范围                                       |
| ---------- | ---------------------------------------- | ------------------------------------------------------ | ---------------------------------------------- |
| HTTP       | 健康 GET 处理器                          | 此次请求得到后端响应                                   | 读取本地状态和缓存，不等待数据库或 Worker 探测 |
| 数据库     | 现有连接池中的一次实际表读取             | 在检查预算内完成读取                                   | 不保证后续写入成功或全库无损坏                 |
| 新提交接纳 | 创建服务停止标记、Scheduler 状态和信号量 | 生成快照时是否停止新建、是否还有待接纳名额             | 实际 POST 仍执行原有校验和名额预留             |
| Scheduler  | 已有 `stopping`、`closed` 标记           | 本地调度任务尚未结束，或正在停止                       | 不增加调度进度心跳或卡死判定                   |
| Kafka      | 对配置的任务及事件 topic 查询 metadata   | 请求完成且 topic 分区元数据可用                        | 不保证发送、消费或端到端判题成功               |
| Worker     | 状态 RPC                                 | 此次通信成功，以及远端报告的执行占用、停机和结果区状态 | 不保证下一次提交必然接纳或实际沙箱执行成功     |

队列满和 Worker 忙属于容量状态。Worker RPC 或 Kafka metadata 健康检查失败本身不限制接纳；新建另行要求独立成员缓存中存在新鲜正成员数，再按 Scheduler 容量判断。成员数不表示空闲位置，健康页面不参与接纳决策。

需要验证编译器、解释器、Isolate、配额和结果保存的完整链路时，继续使用[已有实际判题验收](../crates/backend/README.md#检查命令)。健康 GET 不创建试判任务，不重复工具检查或沙箱初始化。

## 3. Worker 状态 RPC

### 3.1 协议增量

在现有 `acorn.worker.v1.JudgeWorker` 中追加以下 RPC 和消息，已有消息字段及编号保持不变：

```proto
rpc GetWorkerStatus(GetWorkerStatusRequest) returns (GetWorkerStatusReply);

message GetWorkerStatusRequest {}
message GetWorkerStatusReply {
  bool stopping = 1;
  WorkerExecutionState execution_state = 2;
  uint64 unacknowledged_results = 3;
  uint64 unacknowledged_capacity = 4;
}
enum WorkerExecutionState {
  WORKER_EXECUTION_STATE_UNSPECIFIED = 0;
  WORKER_EXECUTION_STATE_IDLE = 1;
  WORKER_EXECUTION_STATE_RUNNING = 2;
  WORKER_EXECUTION_STATE_BLOCKED = 3;
}
```

`execution_state` 的定义如下：

| 状态      | 来源                                     |
| --------- | ---------------------------------------- |
| `IDLE`    | `Jobs.execution` 为空                    |
| `RUNNING` | 执行位置存在，尚未因停止未确认而保留     |
| `BLOCKED` | Job 收尾后无法确认停止，执行位置继续保留 |

`stopping` 直接读取 `Jobs.stopping`。`unacknowledged_results` 读取 Kafka 驱动写入共享状态的待 MQ 交接终态数，包含 FIFO 排队及正在交接的条目，也包含未进入运行记录表的无效任务终态和旧归属终态。执行中的预留名额不计入；交接完成后移出计数。`unacknowledged_capacity` 读取 `results.max_unacknowledged_results`；字段名及编号不变，查询不改变队列或计数。

正常状态均返回成功，包括忙、结果区满、执行位置阻塞和停机中。只有 RPC 本身失败才返回 gRPC 错误。消息不包含 Job ID、源码、测试数据、工具路径或诊断正文。

### 3.2 最小实现

`jobs::Execution` 表示 Worker 唯一的执行位置。当前代码已经区分两种收尾：确认沙箱执行已停止时释放位置；无法确认停止时保留位置，并拒绝后续新 Job。后者的 Job 可以已经发布失败结果，但执行位置仍被占用。

新增 `stop_unconfirmed: bool` 只用于把这两类占用区分出来：新接纳时为 `false`，对外显示 `running`；执行器返回且 `is_available()` 为 `false` 时设为 `true`，对外显示 `blocked`；能够安全释放时仍删除整个执行位置，对外显示 `idle`。

设置该字段和发布终态在同一次 `jobs` 加锁期间完成，使状态查询取得一致的结果与占用信息。

该字段随执行位置保留，不依赖已交接释放的终态记录。原有停止、结果保存和执行位置释放规则保持不变，不另建恢复状态机。

RPC 在现有 `jobs` 短同步锁内取得快照，不获取执行器的异步锁，不等待当前 Job。忙时也必须能够查询。

后端仅接受已定义且非 `UNSPECIFIED` 的执行状态、正值容量及不超过容量的未确认数量。违反这三项协议条件时记录 `InvalidWorkerStatus`，本次观测失败；不增加内容探测或替代 RPC。

## 4. 后端观测来源与采集

### 4.1 本地状态

创建服务提供读取其停止标记的方法。Scheduler 提供本地快照，包含生命周期和待接纳名额；快照复用现有短锁及信号量，不另存容量计数。

| HTTP 字段                    | 定义                                                                                                                               |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `scheduler.state`            | `closed` 为真时为 `stopped`；否则 `stopping` 为真时为 `stopping`；其余为 `running`                                                 |
| `scheduler.pending_capacity` | `scheduler.pending_max_count`                                                                                                      |
| `scheduler.pending_used`     | 容量减去 `Semaphore::available_permits()`，包含创建预留、排队和等待接纳确认的任务                                                  |
| `submission_admission`       | Scheduler 已结束时为 `unavailable`；否则创建服务或 Scheduler 已停止新接纳时为 `stopping`；否则无剩余许可时为 `full`；其余为 `open` |

本地状态在每次健康 HTTP 请求中读取。不同组件的读取不是跨组件事务，快照不预留容量，也不承诺随后 POST 的结果。已有同键提交的返回规则仍以创建接口为准，不能从 `full` 推导所有 POST 都失败。

本版不导出节点调度缓存、提交到 Worker 的映射或新的在途计数；Worker 状态由 RPC 提供。

### 4.2 数据库读取

`Database::probe_read()` 复用共享 MySQL 有界连接池和现有只读错误观察边界，执行：

```sql
SELECT id FROM users LIMIT 1;
```

它检查一次实际读取能否在预算内完成，包括从连接池取得连接和执行 SQL。MySQL 读取包含必要的网络交互；只读网络失败结束本轮观测，不能据此断言服务端存储损坏。“复用只读错误观察边界”指继续通过 `Database::observe` 记录和分类错误，无需另外实现一套异常处理。

使用可选行查询，空表同样表示读取成功；本接口不重新检查预置用户或结构版本。超时覆盖连接池等待和查询，表示在预算内没有取得读取结果，不保证服务端在同一时刻终止工作。MySQL 连接取得和会话锁等待仍分别受其配置约束，见[迁移配置合同](09-mysql-migration-design.md#31-配置密码与连接池)。

普通读取错误只影响本次观测。分类从实际 MySqlDatabaseError 取得数值错误号，普通网络 I/O 和池等待不据此认定存储损坏；服务端明确的严重存储错误仍通知既有故障退出流程，映射见[迁移错误合同](09-mysql-migration-design.md#6-错误与健康观测)。共享数据库句柄保留首个致命通知，通知不依赖健康 HTTP 请求的存续时间；健康模块不吞掉错误，也不新增退出或修复规则。

### 4.3 共享探测任务

新增 `backend::health` 模块，持有数据库、Kafka 及各 Worker 的最近一次探测结果。缓存通过 `Arc<Mutex<...>>` 和短临界区共享，数据库及网络等待均在锁外；HTTP 只读取这份缓存，不发起 SQL、Kafka 请求或 Worker RPC。

`Scheduler::restore` 为每个配置地址构建一个只读 `WorkerClient`，健康采集复用其 channel。它同时构建任务生产者，任务发布、Kafka metadata 健康检查和独立成员观测共用该客户端的廉价克隆；后端不加入 Worker 任务组。

一个后台任务按以下顺序工作：

1. HTTP 开放时缓存可以尚无结果；后台立即开始第一轮。
2. 每轮并发执行一次数据库读取、一次 Kafka metadata 检查和对每个配置 Worker 的一次 `GetWorkerStatus`。使用已有 Tokio `JoinSet`，复用 `WorkerClient` 的 channel，不为健康查询重新建连接。
3. 数据库使用异步超时，Worker RPC 同时携带同一预算的 gRPC deadline。Kafka 同步查询通过 `JoinSet::spawn_blocking` 运行，两个配置 topic 共用一次总预算，各 SDK 调用只取得剩余时间；检查 topic 存在、分区非空、分区错误及 leader 可用性，不发送业务或探测消息。每项完成后更新该项缓存和完成时间。
4. 本轮全部结束后等待 `interval_ms`，再开始下一轮。同一对象不重叠探测，没有单轮重试或失败后的备用路径。

配置只增加两个值：

```toml
[health]
interval_ms = 5_000
timeout_ms = 1_000
```

两值均为正数，`timeout_ms` 不超过 SDK 正整数毫秒上限 `2147483647`；缺省采用上述值，旧后端配置仍可加载。

过期阈值表示缓存结果可以作为近期观测展示的最长时间，不是数据库或 Worker 调用超时，也不是 Job 执行期限。阈值固定派生为 `2 * (interval_ms + timeout_ms)`，默认 12 秒：按一轮约 1 秒检查、随后等待 5 秒估算，允许约两个周期没有新结果。配置校验拒绝该计算溢出，不增加独立的过期阈值配置。

探测调用失败时立即记录失败，不等待过期阈值。最近结果的年龄达到阈值时显示 `stale`，例如探测任务异常结束后未再更新；页面按第 6 节提供独立的过期警告。

耗时、超时和过期判断使用单调时钟。对外完成时间使用后端 UTC Unix 毫秒，不依赖 Worker 时钟。取时间失败时记录明确错误并结束探测任务，不伪造时间戳。

探测任务是后端进程内的一个 Tokio 后台任务。它的生命周期由程序入口负责：

1. 单次数据库、Kafka 或 Worker 检查失败时，把错误写入该项缓存，探测任务继续下一轮。这属于正常的检查结果。
2. 探测任务本身异常结束时，例如发生 panic，入口记录错误；判题服务继续按原契约运行，不自动重建探测任务。缓存不再更新，按过期阈值随后显示 `stale`。严重数据库错误仍由已有存储故障通知触发业务退出，不被此规则覆盖。
3. 正常停机时，后端先停止新提交和新分派，等待已开始的业务操作；这段时间 HTTP 查询仍可用，健康探测也可继续。HTTP 服务结束后，入口通知探测任务停止，等待其结束后再关闭共享数据库。
4. 后端因已有业务故障退出时，同时通知探测任务停止。停止信号中断异步数据库／RPC 等待及轮次间隔；已经运行的阻塞 Kafka 查询继续受 SDK 有限超时约束，采集器等待其结束，不把取消等待当作底层查询取消。停止后不启动新一轮。

这里停止的是健康采集任务，不是 Worker 的判题任务；不发送取消 Job、释放执行位置或消息交接的操作。

### 4.4 缓存状态和错误码

数据库、Kafka 和每个 Worker 独立记录观测，HTTP 字段采用相同含义：

| `state`   | `checked_at_ms` | `error_code` | Worker `status` |
| --------- | --------------- | ------------ | --------------- |
| `unknown` | `null`          | `null`       | `null`          |
| `ok`      | 最近完成时间    | `null`       | 有效快照        |
| `failed`  | 最近完成时间    | 下表错误码   | `null`          |
| `stale`   | 最近完成时间    | `null`       | `null`          |

尚无结果时为 `unknown`。每次探测失败立即覆盖旧成功结果，不能继续显示旧的空闲状态。最近结果的单调时间年龄达到或超过过期阈值时，对外投影为 `stale`，不再输出旧 Worker 状态；内部仍只保留最近一条结果。数据库的 `ok` 表示读取成功。

| 错误码                    | 来源                                                    |
| ------------------------- | ------------------------------------------------------- |
| `ProbeTimeout`            | 数据库、Kafka 或 Worker 调用超出本次预算                |
| `KafkaMetadataFailed`     | Kafka metadata 请求失败，或配置 topic／分区元数据不可用 |
| `DatabaseReadFailed`      | 数据库实际返回读取错误                                  |
| `WorkerRpcFailed`         | Worker RPC 返回其他通信或服务错误                       |
| `WorkerStatusUnsupported` | Worker 返回 `UNIMPLEMENTED`                             |
| `InvalidWorkerStatus`     | 回复违反第 3.2 节的协议条件                             |

`failed` 表示本次检查没有取得有效状态，不断言 Worker 进程已死亡。旧 Worker 不支持新 RPC 时明确显示接口不支持，不尝试用假 Job 或其他调用补齐状态。原始数据库、Kafka 和 RPC 错误保留在英文服务端日志中，HTTP 只返回稳定错误码；正常探测日志使用 debug。

## 5. HTTP 契约与可见范围

### 5.1 健康快照

`GET /api/v1/health` 成功取得快照时返回 `200`，即使组件处于 `failed`、`stale`、`full` 或 `stopping`。它供 GUI 展示各维度，不使用 HTTP 状态码替代组件状态，不增加整体健康布尔值或 readiness 接口。

本接口直接读取本地状态和缓存，数据库、Kafka 或 Worker 的单次探测失败不妨碍它返回响应。目前没有独立存活探针的调用方，因此不另设 `/api/v1/health/live`；本次 HTTP 是否响应由调用结果体现。

```json
{
  "snapshot_at_ms": 1700000005000,
  "submission_admission": "open",
  "scheduler": {
    "state": "running",
    "pending_used": 0,
    "pending_capacity": 128
  },
  "database": {
    "state": "ok",
    "checked_at_ms": 1700000004000,
    "error_code": null
  },
  "kafka": {
    "state": "ok",
    "checked_at_ms": 1700000004000,
    "error_code": null
  },
  "workers": [
    {
      "id": "worker-1",
      "state": "ok",
      "checked_at_ms": 1700000004000,
      "error_code": null,
      "status": {
        "stopping": false,
        "execution_state": "idle",
        "unacknowledged_results": 0,
        "unacknowledged_capacity": 128
      }
    }
  ]
}
```

`execution_state` 转换为 `idle`、`running`、`blocked`。所有示例字段均为固定响应字段；探测字段的 `null` 规则见第 4.4 节，本地状态和容量字段始终来自实际读取。`snapshot_at_ms` 是 HTTP 组装时间，各 `checked_at_ms` 是该项探测完成时间，不能混用。

数据库、Kafka 和各 Worker 并发探测、各自更新，所以完成时间可能不同。保留各自的 `checked_at_ms`，使用户能看到指标是哪次检查取得的；即使刚收到 HTTP 响应，也不能据此把缓存中的所有指标视为刚完成检查。时间字段只随最近结果保存在内存，不建立历史记录。

### 5.2 可见范围

Worker 按配置顺序返回，标识为 `worker-1`、`worker-2` 等；配置顺序变化后标识可变化，不承担持久身份职责。响应不包含内部 endpoint、数据库路径、源码、测试数据或原始异常。

接口返回 `Cache-Control: no-store`。无法组装响应时沿用现有 `500 InternalError` 结构；路由和方法错误沿用既有规约。前端已有的请求超时、取消和非 JSON 错误处理继续适用。

访问范围沿用现有同源 API 和部署访问边界，当前无应用鉴权。本版只公开以上摘要；若部署要求只有运维人员能访问，应由现有反向代理限制健康路径，或另行设计鉴权，不顺带增加角色系统。

此处“公开摘要”指浏览器通过现有 API 能读取的字段。页面展示节点标识、状态、数量、检查时间和稳定错误码；内部地址和原始错误继续用于服务端定位问题。“独立页面”是同一个前端工程内的 `/health` 路由；“手动刷新”是用户点击后重新 GET 已有缓存，不立即触发新一轮探测。

## 6. 前端接入

核心 GUI 交付后增加 `/health` 页面和“系统状态”导航。页面进入时立即 GET 健康快照，此后在页面挂载期间每 10 秒自动获取，也保留手动刷新。两种刷新共用同一查询；定时到达时若请求尚未结束则跳过该轮，请求失败后下轮继续获取。离页时清理定时器、取消在途等待并忽略迟到响应。复用 F02 的查询生命周期、加载和错误提示，前端只访问后端 HTTP 缓存，不直接连接 Worker，也不触发新一轮后端探测。

页面按后端维度展示，注明“获取时快照”和获取时间、各项检查时间。数据库 `ok` 显示“读取正常”，Kafka `ok` 显示“元数据可用”；Worker `ok` 根据 `status` 显示执行状态、是否停机和待 MQ 交接结果数量。

数量列名为“待 MQ 交接结果 / 容量”；未知、失败或过期时显示未取得有效状态，不显示为零。前端直接展示 Worker 的执行状态、停机标记、待 MQ 交接结果数量与容量，以及探测状态和检查时间。不组合这些指标生成“报告可接纳”、节点健康或整体可用的结论，也不增加合成状态灯。

`unknown`、`failed`、`stale` 分别显示尚未取得结果、检查失败及观测已过期，并提供中文错误码映射。HTTP 刷新失败保留上次快照并明确标记为旧数据；首次失败显示错误和重试。组件状态不用于禁用答题提交按钮，实际创建错误仍由既有提交流程处理。

健康页面在指标区域上方设置独立的观测过期警告区，复用 Element Plus 的 `ElAlert`。每次取得快照时，只要数据库、Kafka 或任一 Worker 的 `state=stale`，就显示“以下观测信息已过期，请刷新后查看检查结果”，列出过期对象及各自最后检查时间；刷新取得非过期状态后移除对应对象，全部恢复后隐藏警告区。该提示只说明数据的新旧，不推导组件是否可用。

警告区随页面进入、每 10 秒自动获取和手动刷新取得的后端 `stale` 状态更新；前端不增加本地观测过期计时器。HTTP 请求失败仍通过原有请求错误区提示，与观测过期警告分开呈现；保留上次成功快照及其过期警告，后续获取成功后按新状态更新。

## 7. 可执行实施步骤

F08 按以下顺序划分任务，依赖核心 GUI 的接口与查询能力。每步只运行受影响的检查和测试，阶段结果写入[实施记录](d02-execution/06-frontend-development-execution.md#5-实施记录)。

| 步骤                | 修改位置与操作                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | 完成条件                                                                                                   |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| H01 Worker 状态     | 在[协议源文件](../proto/acorn/worker/v1/worker.proto)追加第 3 节协议；在[Jobs](../crates/worker/src/jobs.rs)增加执行位置标记；在[RPC](../crates/worker/src/rpc.rs)实现查询并接入现有终态发布。同步[Worker 设计](01-worker-design.md#3-rpc-契约)                                                                                                                                                                                                                                             | 生成客户端可查询空闲、忙、阻塞、停机和结果区数量；查询不改变业务状态                                       |
| H02 观测数据与采集  | 在[后端配置](../crates/backend/src/config.rs)增加有缺省值的 `health` 配置并更新[示例](../config/backend.example.toml)；在[创建服务](../crates/backend/src/submissions/creation.rs)和[Scheduler](../crates/backend/src/scheduler/mod.rs)增加只读本地快照；在[数据库](../crates/backend/src/database/mod.rs)增加读取探测；在[Worker 客户端](../crates/backend/src/scheduler/worker_client.rs)增加状态调用。新增 `crates/backend/src/health.rs` 并从[库入口](../crates/backend/src/lib.rs)导出 | 单轮有界探测可更新各项缓存，查询不影响任务分派、名额和消息交接                                             |
| H03 HTTP 与生命周期 | 在[HTTP 路由及 DTO](../crates/backend/src/http/mod.rs)增加第 5 节接口；在[程序入口](../crates/backend/src/main.rs)组装、观察和结束共享探测任务；同步[后端设计](03-backend-design.md)的接口、配置和模块职责                                                                                                                                                                                                                                                                                  | HTTP 读取本地状态和缓存；数据库普通读取失败或 Worker 无响应时仍能返回组件快照；停机按第 4.3 节收尾         |
| H04 GUI 与说明      | 在[前端 API](../web/src/api/index.ts)及 DTO 增加接口；新增[健康页面](../web/src/pages/HealthPage.vue)，更新[路由](../web/src/router.ts)和[导航](../web/src/App.vue)；复用 `useQuery` 接入每 10 秒自动获取，复用 `ElAlert` 增加独立过期警告区。同步前端设计、F08、根说明及前端说明                                                                                                                                                                                                           | 实际页面直接展示指标，自动获取、手动刷新、请求错误和观测过期警告可用；离页清理查询；文档准确记录已执行范围 |

H01 修改 Proto 后通过 Cargo 构建生成代码，不手工编辑 `OUT_DIR`，所有实现 `JudgeWorker` 的现有受控服务需补齐新方法，默认返回确定的受控状态即可。H02 复用现有客户端 channel；H03 更新现有组装调用和测试夹具，不保留第二套无健康功能的组装路径。

H01–H03 按实际变更执行已建立的命令：

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-worker --locked --test kafka real_kafka_executable_handles_sigterm -- --ignored
cargo test -p acorn-backend --locked --test workflow
```

Worker RPC 测试用于 H01；后端 workflow 测试用于 H02–H03，若在已有模块中加入定向单元测试，再执行其具体筛选命令。只运行新增行为所在的测试目标，不要求每一步重跑所有命令或完整 workspace 测试。真实 socket 测试需要可绑定回环端口的环境。

H04 在 `web/` 执行已有 `pnpm format:check`、`pnpm lint`、`pnpm typecheck`、`pnpm build` 和相关 `pnpm test`。全部 Rust 自写项继续遵守 Rustdoc 和 workspace lint 要求。

## 8. 必要验收与验证边界

复用现有 RPC、MySQL、HTTP 和前端查询夹具，只为下列新增契约补断言，不建立健康测试框架或故障矩阵。

| 标识  | 必须验证的行为                                                                                                                                                                         | 最小验证方式                                                                                                                                                                        |
| ----- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| HC-01 | Worker 查询在忙时可返回；停止未确认且结果淘汰后仍为 `blocked`；结果区数量及停机标记正确；查询不改变保留                                                                                | 扩展现有忙拒绝、结果区满、停止未确认和停机 RPC 测试，在既有节点插入状态查询；沿用事件同步，不断言毫秒级性能                                                                         |
| HC-02 | `unknown → ok → failed → ok` 反映最近探测；年龄达到阈值时输出 `stale` 且不输出旧 Worker 状态；不支持 RPC 和无效状态有明确错误码                                                        | 在同一受控后端夹具切换返回或关闭／恢复监听；时间过期在模块测试直接传入指定单调时间，不实际等待 12 秒。枚举和数量校验用一组表驱动用例覆盖三项条件                                    |
| HC-03 | `pending_used` 包含已预留但尚未入队的名额；满时为 `full`；停止接纳时为 `stopping`；Worker 失败不关闭本地接纳；多个健康 GET 不触发新探测                                                | 在现有容量和正常停机 workflow 测试中补 HTTP 断言；固定已完成缓存并比较 GET 前后的探测调用次数，不另跑并发压测                                                                       |
| HC-04 | 数据库普通读取失败或等待超时时只降低观测；健康接口仍能读取本地缓存                                                                                                                     | 扩展现有普通读取锁等待用例，查询健康接口并保留原有进程继续运行断言；严重错误分类复用现有覆盖                                                                                        |
| HC-05 | GUI 区分组件失败、容量状态和 HTTP 请求失败；独立警告区列出过期对象并在刷新恢复后清除；每 10 秒自动获取，忙时跳过，失败保留旧快照且下轮继续；手动刷新可用；离页清理定时器并忽略迟到结果 | 复用 F02 查询生命周期及健康状态映射测试；用一个桌面浏览器及受控响应检查自动获取、失败后的下轮恢复、忙时跳过、手动刷新、离页清理和 `stale → ok` 警告恢复，不重复题目编辑／判题主路径 |

完成后做一次实际连通检查：按已有说明启动同版 Worker 和后端，查询健康接口，打开健康页面；停止一个空闲 Worker，在下一次探测完成后手动刷新，确认该节点检查失败而 HTTP 仍响应，再启动该 Worker 并确认后续探测恢复。该检查不创建提交，不要求双 Worker、真实磁盘满、完整沙箱隔离或所有 verdict 验收。

新 RPC 通过实际生成客户端和服务端验证，不另外编写只重复 Prost 编解码的往返测试。现有启动、消息交接、存储严重错误和取消行为继续使用已有覆盖，除非实施中发现与本次改动直接相关的问题，否则不扩展测试范围。

文档变更检查格式、稳定编号和本地链接。初次实际执行结果和未测范围见[健康页面实施记录](d02-execution/06-frontend-development-execution.md#511-f08h04健康页面与最终连通验收)，后续自动获取的实现与验证边界见[补充记录](d02-execution/06-frontend-development-execution.md#512-健康页面每十秒自动获取)。

## 9. 记录入口

- [审核记录](d01-review/07-health-observation-review.md)：范围与观测职责的确认结论。
- [实施记录](d02-execution/06-frontend-development-execution.md#58-f08h01worker-状态-rpc)：H01–H04、周期获取及对应检查。

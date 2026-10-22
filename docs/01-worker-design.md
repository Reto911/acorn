# Worker 设计

本文定义独立 Worker 的 gRPC、串行判题、语言计划、沙箱隔离、结果保留和资源边界。只读状态 RPC 与后端采集、HTTP 及健康页面的分工见[健康观测设计](07-health-observation-design.md)。

基础判题的实际过程与检查见[Worker 与后端实施记录](d02-execution/04-development-execution.md#7-实施记录)，状态 RPC 和健康页面的交付见[前端与健康观测实施记录](d02-execution/06-frontend-development-execution.md#58-f08h01worker-状态-rpc)。

## 1. 目标与范围

Worker 是独立运行的判题进程，提供 unary gRPC 服务。Scheduler 推送任务、轮询进度、获取结果并保存。一个 Worker 同时只执行一个 Job，一个 Job 的全部阶段和测试用例依次执行。

| 项目 | 首版设计 |
| --- | --- |
| 分配 | Scheduler 推送；忙时拒绝，Worker 不排队 |
| 通信 | 短 RPC；提交立即确认，结果通过轮询获取 |
| 语言 | C 与 Python 3，所有 Worker 使用相同的语言约定 |
| 去重 | 只以 Job ID 为依据，已有记录直接返回，不比较任务内容 |
| 执行 | C 编译一次，Python 直接解释执行；用例串行，首个失败用例立即短路 |
| 沙箱 | 编译和每个用例分别使用新沙箱；前一个阶段进程树停止并完成收尾调用后才创建下一个 |
| 程序文件 | C 产物或 Python 源码保存在 Job 私有上下文，每个运行沙箱获得独立副本 |
| 清理 | 文件或已停止环境的清理失败只记日志，不改变判题结果，继续接纳任务 |
| 结果 | AC 不返回用例，CE 只返回编译诊断，用例失败只返回首个失败用例 |
| 保留 | 未确认获取的终态记录留在内存，确认后进入定长 FIFO 队列 |
| 重试 | 首版不自动重试；以后重试由 Scheduler 创建新 Job ID，并可选择任意空闲 Worker |
| 恢复 | 不落盘、不恢复任务，不保证重启后的结果保留和去重 |
| 取消 | 无远程取消 RPC；保留超限、故障和停机所需的内部停止能力 |
| 工具链 | 部署端配置 C 编译器与 Python 解释器；不计算摘要、不固定版本、不保证跨 Worker 一致性 |
| 鉴权 | 不鉴权，任何能连接服务且请求符合规约的调用方均可调用 |

不使用 Lease、Attempt、Worker 能力协商或进程 Instance ID。Worker 不访问业务数据库，也不提供任务恢复、残留资源修复或配置动态重载功能。

Worker 继承启动它的用户身份，不创建账号，不内置切换用户逻辑。沙箱后端所需的权限与运行身份由后端自身及部署端安排。

## 2. 职责与任务标识

| 组件 | 职责 |
| --- | --- |
| API | 使用配置的测试用户归属、选取最新题目版本、保存提交、将任务数据交给 Scheduler、提供提交历史查询；当前不登录鉴权 |
| Scheduler | 作为后端进程内模块维护内存队列、使用 Submission ID 作为 Job ID、选择 Worker、提交、轮询、保存结果并确认获取、判定失联失败 |
| Worker | 去重与接纳、选择语言计划、组织可选编译和串行运行、比较输出、保存内存记录、输出日志 |
| 语言适配器 | 根据本地配置生成固定源码名、可选编译计划和运行计划 |
| 沙箱封装 | 初始化、传入文件、施加限额、执行、收集输出和产物、停止进程树、调用清理 |
| 部署端 | 安装 C/Python 工具链和沙箱、配置权限与资源、收集日志、处理残留文件或环境问题 |

Submission 是业务提交，Job ID 是一次执行的标识。首版一个 Submission 最多对应一个 Job ID，可以直接使用唯一的 Submission ID，不要求额外建立 Job 数据表。

API 创建 Submission 时生成 UUID，Scheduler 直接使用它作为 Job ID，不由源码摘要生成。Worker 将其视为不透明字符串。相同源码的不同提交应有不同 ID；同 ID 的重复请求直接返回已知状态，即使携带的内容不同也不重新执行。

正常提交由 API 直接向 Scheduler 传递源码及固定版本的任务数据，Scheduler 将待执行任务保存在内存中，不轮询数据库队列或为同一新任务再次读取源码。重要业务状态与终态仍持久化；启动时从数据库装入尚未分派的任务，见[数据库设计](02-database-design.md)。

API 与 Scheduler 在同一后端进程中，通过进程内调用和消息交接任务。后端通过 HTTP JSON 与前端交互，通过下文的 gRPC 契约调用独立 Worker，见[后端与 Scheduler 设计](03-backend-design.md)。

去重只覆盖本进程仍保留的记录。记录被淘汰或进程重启后，不再提供该 ID 的去重保证。Scheduler 不重新提交已经结束、失败或确认状态不明的旧 Job。以后重新判题或自动重试时使用新 ID，并可分派到任意空闲 Worker。

## 3. RPC 契约

### 3.1 服务

| RPC | 行为 |
| --- | --- |
| `SubmitJob` | 接纳任务并立即返回，不等待执行完成 |
| `GetJob` | 立即返回阶段、进度或终态，不长轮询 |
| `AcknowledgeResult` | Scheduler 保存终态后确认获取，允许该记录进入淘汰队列 |
| `GetWorkerStatus` | 返回执行占用、停机标记及未确认结果数量和容量，不等待当前 Job |

不要求提交前探测 Worker 状态。Scheduler 根据本地分派记录和可用状态缓存选择 Worker，最终以 `SubmitJob` 的接纳或拒绝为准。gRPC 可以复用传输连接，业务不依赖持续在线的连接。

`GetWorkerStatus` 从现有 `Jobs` 短同步锁内取得一致快照，不获取执行器的异步锁。忙、结果区满、执行位置阻塞和停机中均正常返回成功；查询不改变计数、结果保留或执行占用，不参与分派或接纳决策。返回状态不保证下一次提交能接纳或实际沙箱执行成功，也不提供工具能力列表。字段含义和有限验收见[健康观测设计](07-health-observation-design.md#3-worker-状态-rpc)。

首版不进行调用方鉴权，不要求 token、身份 metadata 或客户端证书。所有调用方使用相同的协议校验与接纳规则。协议版本由 `acorn.worker.v1` 服务名表达，不增加请求版本字段或目标进程身份字段。

### 3.2 消息

```proto
syntax = "proto3";
package acorn.worker.v1;

service JudgeWorker {
  rpc SubmitJob(SubmitJobRequest) returns (SubmitJobReply);
  rpc GetJob(GetJobRequest) returns (GetJobReply);
  rpc AcknowledgeResult(AcknowledgeResultRequest) returns (AcknowledgeResultReply);
  rpc GetWorkerStatus(GetWorkerStatusRequest) returns (GetWorkerStatusReply);
}

message SubmitJobRequest {
  string job_id = 1;
  string language_id = 2;
  bytes source = 3;
  repeated TestCase cases = 4;
  RunLimits run_limits = 5;
}
message TestCase {
  bytes input = 1;
  bytes expected_output = 2;
}
message RunLimits {
  uint64 cpu_time_ms = 1;
  uint64 wall_time_ms = 2;
  uint64 memory_kib = 3;
}
message SubmitJobReply {
  string job_id = 1;
  JobState state = 2;
  bool duplicate = 3;
}
message GetJobRequest { string job_id = 1; }
message GetJobReply {
  string job_id = 1;
  JobState state = 2;
  JobPhase phase = 3;
  uint32 completed_cases = 4;
  uint32 total_cases = 5;
  oneof terminal {
    JudgeResult result = 6;
    JobFailure failure = 7;
  }
}
message AcknowledgeResultRequest { string job_id = 1; }
message AcknowledgeResultReply {}

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

enum JobState {
  JOB_STATE_UNSPECIFIED = 0;
  JOB_STATE_RUNNING = 1;
  JOB_STATE_FINISHED = 2;
  JOB_STATE_FAILED = 3;
}
enum JobPhase {
  JOB_PHASE_UNSPECIFIED = 0;
  JOB_PHASE_PREPARING = 1;
  JOB_PHASE_COMPILING = 2;
  JOB_PHASE_RUNNING_CASE = 3;
  JOB_PHASE_CHECKING = 4;
  JOB_PHASE_DONE = 5;
}
message JudgeResult {
  Verdict verdict = 1;
  optional FailedCase failed_case = 2;
  bytes compile_diagnostic = 3;
  bool compile_diagnostic_truncated = 4;
}
message FailedCase {
  uint32 case_index = 1;
  bytes stdout = 2;
  bytes stderr = 3;
  bool stdout_truncated = 4;
  bool stderr_truncated = 5;
  ResourceUsage resources = 6;
  oneof exit {
    uint32 exit_code = 7;
    uint32 signal = 8;
  }
}
message ResourceUsage {
  optional uint64 cpu_time_us = 1;
  optional uint64 wall_time_us = 2;
  optional uint64 peak_memory_bytes = 3;
}
enum Verdict {
  VERDICT_UNSPECIFIED = 0;
  VERDICT_ACCEPTED = 1;
  VERDICT_WRONG_ANSWER = 2;
  VERDICT_COMPILE_ERROR = 3;
  VERDICT_RUNTIME_ERROR = 4;
  VERDICT_TIME_LIMIT_EXCEEDED = 5;
  VERDICT_MEMORY_LIMIT_EXCEEDED = 6;
  VERDICT_OUTPUT_LIMIT_EXCEEDED = 7;
}
message JobFailure {
  string code = 1;
  string diagnostic = 2;
}
```

源码、输入和答案直接放入有大小上限的请求，不使用外部下载地址或宿主文件路径。测试用例的数组顺序就是执行顺序，`case_index` 从 1 开始；不额外设置用例 ID 和排序字段。

`language_id` 首版接受 `c` 和 `python`，后者表示 Python 3。输出比较规则固定，不由请求选择 checker。题目要求的单用例 CPU、墙钟时间和内存通过 `run_limits` 提交；Worker 配置其允许上限。编译预算、输出上限和其他门限由 Worker 配置文件提供。

内存限额使用正整数 `memory_kib`，单位为 KiB（1 KiB = 1024 字节），从题库到 RPC、语言计划和 Isolate 参数保持同一单位。当前开发阶段直接将字段号 3 的名称和单位改为 `memory_kib`，不保留旧字段或兼容处理；后端与 Worker 按当前协议同步构建运行。实际资源统计 `peak_memory_bytes` 仍使用字节。

`FINISHED` 表示取得判题结论，包含一个 `JudgeResult`；`FAILED` 表示执行基础设施或 Job 总期限导致失败，包含 `JobFailure`。CE、WA、RE、TLE、MLE、OLE 都是判题结论，不是基础设施失败。运行中不包含终态消息。

AC 仅返回结论；CE 返回结论及有界编译诊断，不返回 `failed_case`；用例级错误返回结论及唯一的首个失败用例。输入和答案已在 Scheduler 中，不在结果中重复发送。未运行用例不生成占位记录。`completed_cases` 统计已经得到裁决的用例，包括首个失败用例；CE 时为 0。

`failed_case` 显式声明为 `optional`，用字段存在性区分“没有失败用例”和“包含失败用例”。Proto3 的单个 message 字段本身也具有存在性，见[Protobuf 字段存在性文档](https://protobuf.dev/programming-guides/field_presence/)。

### 3.3 接纳与去重

接纳过程按以下顺序执行：

1. 检查 Job ID 非空且长度在配置范围内。
2. 查询内存记录。ID 已存在时直接返回原状态及 `duplicate=true`，不比较内容，不再次执行，不延长期限。
3. 检查语言、测试用例数量、数据大小和运行限额。
4. 在同一接纳临界区确认执行位置未被占用、未确认结果区还有空间，且不在停机中。
5. 插入 `RUNNING/PREPARING` 记录，标记 Worker 忙，启动后台执行，返回 `duplicate=false`。

步骤 2、4、5 必须保证并发 RPC 不会重复启动任务。RPC 可以同时到达，但执行器始终只有一个 Job。

忙时拒绝发生在步骤 5 之前：不插入该 Job 的记录，不启动沙箱，不将它放进内部队列。只有明确收到未接纳响应的任务，Scheduler 才能继续排队或交给其他 Worker。

| 情况 | gRPC 状态及原因 |
| --- | --- |
| 语言不是 `c` 或 `python` | `INVALID_ARGUMENT`，`UnsupportedLanguage` |
| 必填值、用例数量、大小或限额无效 | `INVALID_ARGUMENT`，`InvalidJob` |
| 执行位置已占用或未确认结果区已满 | `RESOURCE_EXHAUSTED`，`WorkerBusy` / `ResultBufferFull` |
| 停机 | `FAILED_PRECONDITION`，`WorkerUnavailable` |
| Job ID 没有记录 | `NOT_FOUND`，`JobNotFound` |
| 确认仍在运行的任务 | `FAILED_PRECONDITION`，`ResultNotReady` |

校验内容限定为执行所需的基本条件：至少一个用例、数量不超限，源码非空，源码、单个输入、单个答案及合计字节数不超限。空输入、空答案和内容相同的多个用例均允许。时间和内存必须显式提供正值且不超过配置上限；未填值产生的 0 不表示无限额。源码是否有效交给对应的编译器或解释器判断。

### 3.4 内存记录与结果保留

Worker 使用 `HashMap<JobId, JobRecord>` 保存运行状态或终态结果，用 `VecDeque<JobId>` 保存已确认结果的淘汰顺序。这些都是进程内数据结构。

终态写入后不再修改。未确认获取的终态记录不设 TTL，也不因新任务到来被淘汰。当未确认结果数达到配置上限时，Worker 拒绝新任务，已有记录继续可查。

Scheduler 取得终态并成功保存后调用 `AcknowledgeResult`。确认将记录移入定长 FIFO 队列；超过队列长度时删除最早确认的记录。重复确认仍在队列中的记录成功返回，不重新排列顺序。记录尚未进入终态时不能确认。

`GetJob` 本身不改变保留状态。服务端发送成功不能证明 Scheduler 已读取并保存结果，因此只有显式确认才允许淘汰。gRPC 客户端和服务端可以对同一次调用得出不同的完成状态，见[gRPC 调用生命周期](https://grpc.io/docs/what-is-grpc/core-concepts/#rpc-termination)。无论正常判题还是基础设施失败，都使用相同的保留规则。

Job 结束时，若本阶段执行进程尚未启动或进程树已确认停止，释放源码、全部测试数据及编译产物的内存引用，尝试删除私有工作文件，只保留状态和有界结果。文件删除失败记录日志，不阻止后续任务。停止未确认时按第 6 节保留可能仍在使用的资源和执行位置。Worker 不持久化任务或结果，退出或崩溃后的记录保留由本协议明确排除。

[`WorkerService`](../crates/worker/src/runtime.rs) 将接纳、进度和终态的状态转换放在一个短同步锁内；[`jobs`](../crates/worker/src/jobs.rs) 保存执行占用、最新快照通道及旧查询／ACK 记录，[RPC 适配器](../crates/worker/src/rpc.rs)复用同一入口。执行器在锁外后台运行，接纳时建立绝对期限和内部停止通道，不持有提交 RPC 的生命周期。终态在 Job 收尾后原子发布，随后仅在安全停止时释放执行位置；停止未确认时执行占用独立于结果记录保留，即使 ACK 后记录被淘汰也不释放。

[Kafka 服务](../crates/worker/src/kafka/service.rs)通过最新快照通道取得进度及终态，复用同一接纳、执行和停止入口；消费位点、原分区归属、结果 FIFO 及消费者关闭遵循[Kafka 设计第 6 节](10-kafka-judging-design.md#6-worker-消费与执行)。该调用路径不使用 RPC 的结果 ACK 淘汰流程，完整交接后按通道身份释放对应运行记录，避免旧交接删除新的同 ID 执行。当前可执行文件仍选择 RPC 路径；正式切换与旧协议移除由 Kafka 生命周期任务统一处理。

H01 在执行位置保存 `stop_unconfirmed`：接纳时为 false，状态查询返回 `RUNNING`；执行器返回且 `is_available()` 为 false 时，在发布终态的同一次持锁更新中设为 true，返回 `BLOCKED`。安全收尾时删除执行位置，返回 `IDLE`。停机标记直接读取 `Jobs.stopping`，未确认结果数量复用 `Jobs::unacknowledged_count()`，容量来自配置；状态回复不包含 Job ID、源码、测试数据、路径或诊断正文。

## 4. Scheduler 与通信失败

Scheduler 在自己的存储中保存业务终态。提交超时或断连时，不能仅据此认为 Worker 未接纳；可以通过一次有界 `GetJob` 确认。若记录存在则继续跟踪；明确不存在则记为提交失败；无法确认则立即记为 Worker 无响应导致的失败。

正常轮询失败并被判定为 Worker 无响应后，Scheduler 立即保存本次 Job 失败，不设置恢复等待窗口，也不把该 Job 转移给其他 Worker。后台执行可能仍继续，Worker 的本地限额负责约束它。

Scheduler 的失败记录不会被迟到结果改写。如果以后再次取得旧 Job 的终态，可以丢弃其判题内容并确认获取，让 Worker 按正常保留规则回收记录。一次通信故障导致仍在执行的任务被判失败，是当前策略接受的行为。

Worker 重启后查询旧 Job 返回 `NOT_FOUND`；不接管、不恢复旧任务。Scheduler 可在后续新任务中再次选择这个地址，实际接纳仍由 `SubmitJob` 决定，不能把通信失败等同于旧任务已经停止。

Scheduler 重启后，将仍处于提交中或运行中的任务直接保存为失败，不查询或恢复旧执行，不重新提交旧 Job，也不补发旧任务的 ACK。尚未分派的排队任务装入内存继续处理，已保存的终态保持不变。Worker 路由只保存在 Scheduler 内存中，不持久化分派关系或结果确认状态；中断后残留的未确认记录由部署端处理。

首版没有自动重试。以后需要重新执行时，Scheduler 创建新的 Job ID，并可选择任意空闲 Worker。它与旧 Job 分别记录；业务层决定采用哪次新执行的结果。

Scheduler 的轮询间隔和 RPC deadline 放在 Scheduler 配置中。RPC deadline 只约束一次调用；Worker 本地 Job 总期限从接纳开始，覆盖准备、编译、用例执行和比较，不随轮询延长。到期后进入有独立超时的停止和清理路径。

## 5. 串行判题流程

### 5.1 准备程序与可选编译

1. 建立本次 Job 的独立私有工作上下文，按语言选择适配器，生成执行计划。
2. 计划包含编译时，初始化新的编译沙箱，将源码写入适配器指定的固定文件名；C 为 `main.c`。
3. 在沙箱中执行配置的编译器和固定参数，实时收集有界诊断，施加编译限额。
4. 编译成功后，从固定的 `program` 路径取出产物，保存在 Worker 私有上下文。读取时限定为常规文件、不跟随链接，并执行产物大小上限；无需摘要、版本校验或在宿主试运行。
5. 编译进程树停止、产物导出结束后调用沙箱清理，再进入用例阶段。清理报错按第 6 节记录日志。用户代码编译错误或编译资源超限形成 CE，返回编译诊断，不运行任何用例。编译器无法启动等基础设施错误形成 Job 失败。

Python 计划没有编译阶段，跳过步骤 2～5，以原始源码作为待运行文件。不执行独立语法检查或字节码预编译，也不在 Worker 进程中导入或执行提交源码。

编译诊断超过保存上限时继续排空输出，并标记诊断已截断；不因诊断文本较长就判定编译失败。

任务不携带可执行命令、编译选项、宿主路径、环境变量或挂载参数。这些执行配置由部署端提供，Worker 使用参数数组执行，不经 shell。源码内容交给对应工具链处理，不做字符串黑名单。

程序文件只供本次 Job 使用，不建立跨 Job 编译或字节码缓存。Job 工作区使用服务端生成的独立目录名，不直接拼接请求中的 Job ID。沙箱初始化必须提供新环境，固定文件路径不会沿用旧 Job 文件；文件读取约束由沙箱封装的产物导出操作完成。

### 5.2 按顺序运行与比较

程序准备完成后，对请求数组中的每个用例依次执行以下操作：

1. 初始化新的运行沙箱，放入 C 编译产物或 Python 源码的独立副本及当前输入。
2. 按运行计划启动程序或解释器，实时收集有界 stdout/stderr，执行本用例时间、内存及配置中的其他限制。
3. 程序结束或超限后，保存必要输出、退出原因和可用资源统计，停止剩余派生进程并调用沙箱清理。
4. 完成收尾调用后判定运行结果；正常退出时，在 Worker 中比较保存的 stdout 和期望输出。清理警告不改变运行结论。
5. 当前用例 AC 才开始下一个用例；WA、RE、TLE、MLE、OLE 立即结束 Job，返回这个用例。

每个用例只获得当前输入、独立程序文件副本和必要运行时。答案、其他用例数据及 Worker 私有文件不进入沙箱。运行副本的任何修改都不会改变 Worker 保存的文件或后续用例获得的副本。Python 解释器的启动、源码解析和执行时间及内存均计入本用例限额。

运行时持续读取输出是避免阻塞、及时执行输出上限的必要 I/O，不产生多个并行运行的用例。超出 stdout 或 stderr 上限时立即停止，不能只截断后继续判定 AC。

输出比较规则固定为：删除整个字节串末尾的 ASCII 空白 HT、LF、VT、FF、CR、SPACE，再逐字节比较；内部空白保持原样。不提供自定义 checker、Token 或浮点比较。

### 5.3 结论与收尾

| 执行事实 | 结论 |
| --- | --- |
| 编译用户错误或编译阶段资源超限 | CE，无失败用例 |
| 用例 CPU 或墙钟超限 | TLE，返回当前用例 |
| 沙箱确认内存超限 | MLE，返回当前用例 |
| stdout 或 stderr 超限 | OLE，返回当前用例 |
| 非零退出或信号，且无已确认的上述超限；包括 Python 语法错误和未捕获异常 | RE，返回当前用例 |
| 正常退出且输出不等 | WA，返回当前用例 |
| 全部用例正常退出且输出相等 | AC，无失败用例 |
| 沙箱初始化、启动、停止、输出收集或产物导出失败，或 Job 总期限到达 | Job 失败 |
| 已停止环境或 Job 私有文件的清理失败 | 记录警告，不改变判题结论 |

不能仅凭 SIGKILL 推断 MLE。多个已确认的限额同时触发时，固定按 MLE、OLE、TLE 选择结论。可选统计拿不到时保持缺省，不伪造统计值。

最后一个阶段的进程树已确认停止，或执行错误明确发生在本阶段执行进程启动前时，组装有界结果、调用沙箱和私有工作区清理，再写入终态并释放执行位置。清理失败不阻止释放；执行错误在发布终态前形成失败结论，不在结果发布后修改结论。停止未确认时仍发布 Job 失败，但跳过依赖停止的文件清理并保留执行位置，见第 6 节。正常释放后，结果等待获取不占用执行位置；下一 Job 可随后开始，但仍受未确认结果数量上限约束。

P04 的 [`Executor`](../crates/worker/src/executor.rs) 接收已接纳请求、绝对期限与内部停止通道，通过回调发布阶段和已裁决用例数，返回有界终态。Job 上下文保存服务端生成的私有目录、原始请求和编译产物；普通结束删除工作文件并释放数据，停止未确认则保留上下文及沙箱实例，`is_available()` 保持为 false。调用方等待执行完成，不取消执行 future；P05 负责终态登记与执行位置释放。

执行失败代码为 `PreparationFailed`（私有文件或语言准备失败）、`InvalidJob`（内部调用缺少运行限额）、`SandboxError`（已停止或尚未启动的阶段故障）、`JobDeadlineExceeded`（总期限到达）、`WorkerShutdown`（内部停机）和 `StopUnconfirmed`（停止未确认）。诊断最多保留 512 个 UTF-8 字节，包含在配置预留的结果协议空间内；这些代码属于 Job 结果，不是 RPC 拒绝原因。

## 6. 统一沙箱接口

Worker 调用一次完整的阶段执行操作，输入为 `SandboxRunSpec`，输出为 `SandboxRunOutcome`。编译阶段与运行阶段共用同一接口，不向业务层暴露一套独立清理状态机。

| 输入 | 内容 |
| --- | --- |
| 程序 | 部署端配置生成的可执行文件及参数数组、工作目录、有限环境 |
| 文件 | 当前阶段所需的源码或程序副本，以及当前 stdin |
| 隔离策略 | 第 7.2 节规定的目录、设备、网络、环境和 FD 限制，不允许请求放宽 |
| 限额与收集策略 | CPU、墙钟、内存、进程数、FD、单个文件大小、磁盘总量与 inode 配额、输出及停止/清理超时；编译诊断截断保存，运行输出超限停止 |
| 导出 | 编译阶段允许导出的固定产物路径及大小上限；运行阶段无产物导出 |
| 停止信号 | Job 总期限、内部故障或进程正常停机触发的停止请求 |

| 输出 | 内容 |
| --- | --- |
| 执行结果 | 正常退出、退出码/信号，或明确的资源超限原因 |
| 收集内容 | 有界 stdout/stderr、截断标记、可用资源统计、成功编译的产物 |
| 阶段错误 | 初始化、启动、停止、收集或产物导出失败的错误信息；明确区分已停止或尚未启动的执行错误与停止未确认；清理失败由封装记录日志 |

封装内部按“初始化 → 放入文件 → 执行与收集 → 停止进程树 → 导出允许内容 → 尝试清理”完成阶段。正常返回意味着进程树已经结束，输出和产物已保存，清理已经调用；不要求文件删除成功。

P03 的实现位于 [`sandbox` 模块](../crates/worker/src/sandbox/mod.rs)。`IsolateSandbox::run` 串行执行一个 `SandboxRunSpec`，`StageControl` 携带接纳时确定的绝对期限和内部 watch 停止通道。调用方必须等待阶段调用结束，通过通道请求停止，不中止执行 future。`SandboxRunOutcome` 保留独立的 OOM、输出超限和超时事实及原始停止原因，由 P04 选择判题结论；统计仅来自实际元数据，进程树内存使用 `cg-mem`，不以单进程 RSS 代替。`SandboxErrorKind::Execution` 表示尚未启动或已确认停止的基础设施错误；`StopUnconfirmed` 保留元数据工作目录、管理器句柄和执行占用，同一个后端实例拒绝再次执行。

阶段文件名和产物名均为语言计划提供的单个相对文件名。导出时拒绝符号链接、硬链接及非常规文件，并按产物上限独立复制字节。Isolate 的 `--cg-mem`、`--fsize` 和磁盘空间配额均直接使用 KiB，不再由字节除以 1024 换算；1 KiB 是有效的非零限额，0 或超出后端有符号整数范围的值在初始化前明确报错。配置使用 `execution.max_run_memory_kib`、`compile.memory_kib`、`sandbox.file_size_max_kib` 和已有的 `sandbox.disk_quota_kib`。产物导出与输出保留仍按实际字节数计量，比较产物与文件／磁盘限额时将已校验的 KiB 预算乘以 1024。CPU 和墙钟以毫秒转换成小数秒。

Isolate 的 `--run` 不包含 `--cleanup`。封装在执行进程尚未启动或进程树已确认停止后调用 `--cleanup`，覆盖成功、CE、超限和内部错误；初始化成功但后续准备失败时也进入清理路径。停止未确认时跳过产物导出、沙箱清理和依赖停止的私有文件清理，保留可能仍在使用的资源，由部署端处置。这是调用已有清理操作，不追加资源扫描、清理报告核验或其他证明步骤，见[Isolate 官方手册](https://www.ucw.cz/isolate/isolate.1.html)。

CPU、内存及停止操作覆盖本阶段的进程树。Isolate 后端使用控制组模式，编译阶段允许编译器启动必要的子进程；运行阶段按配置限制进程数。

Worker 通过 `Child::wait().await` 等待 `isolate --run` 管理器退出，同时持续收集输出。管理器以退出码 0 或 1 完成执行时，依据 Isolate 的收尾契约确认本阶段进程树已停止，再根据元数据形成阶段结果。这里的退出码属于管理器；提交程序的退出码或信号由元数据表示。Worker 主动停止时向管理器发送 `SIGTERM`，并在 `stop_timeout_ms` 内等待其完成上述退出；管理器被信号终止、等待失败、停止超时或其他无法确认进程树停止的错误均归为停止未确认。`Child::kill()` 发送 `SIGKILL`，不能用作正常停止操作；明确发生在本阶段执行进程启动前的失败按普通基础设施失败处理，见 [Isolate 退出与信号处理实现](https://github.com/ioi/isolate/blob/master/isolate.c)和 [Tokio Child 文档](https://docs.rs/tokio/latest/tokio/process/struct.Child.html)。

内部停止的原始原因由 Worker 保存；管理器因 `SIGTERM` 报告中断时，按原始的输出超限、Job 总期限或停机原因形成对应结论或失败，不将管理器中断直接当作提交程序的 RE。

进程树停止后，后端清理或工作文件删除失败只输出 warn 日志，不改变 Job 结论，不退出 Worker，不暂停接纳。残留工作文件不交给后续任务，部署端负责处理；Worker 不扫描残留资源、不重试清理、不建设通用文件 GC。

下一阶段或下一 Job 仍调用正常初始化路径；初始化成功必须提供独立工作目录和本次资源计量，不能只换目录而沿用旧执行的计量状态。初始化失败则报告该次执行的基础设施错误，不能降级为使用旧环境。Isolate 的固定 box ID 可串行复用，但 `--init` 失败时不得执行程序。以上依赖初始化接口的契约，不追加清理审计或资源扫描。

进程树停止失败或停止未确认属于执行故障，不能按文件清理警告处理。此时发布可查询的 Job 失败终态并保留占用的执行位置；结果获取、ACK 或结果记录淘汰均不释放该位置。Worker 继续提供结果 RPC，拒绝新任务，由部署端处置，避免在旧程序仍运行时启动下一 Job。这依赖后端执行/停止接口的完成契约，不增加独立的 PID 或 cgroup 扫描。

内部停止作用于本次进程树，用于资源超限、Job 总期限、基础设施故障和正常停机。停机时先拒绝新任务，再停止当前执行并调用相同清理路径；不提供客户端取消接口。

Worker 入口接收 Ctrl-C 和 SIGTERM，通过服务的内部停机方法等待已接纳后台任务，受 `shutdown_timeout_ms` 约束；超时不 abort 执行 future，也不宣称停止成功。等待完成后若执行位置仍被保留，停机返回明确错误；正常收尾成功才以状态码 0 退出。

## 7. 语言适配器与隔离边界

### 7.1 语言执行计划

首版内置 C 和 Python 两个适配器，以 `language_id` 查找，不使用动态插件或能力协商。所有 Worker 均部署两种工具链。启动时读取配置并检查必要工具是否可用；无效配置阻止启动。

适配器提供 `plan(config) -> LanguagePlan`，只生成数据，不执行提交代码。计划包含以下内容：

| 字段 | 职责 |
| --- | --- |
| `source_name` | 固定源码文件名 |
| `compile` | 可选编译计划：编译器、参数数组、固定产物名及编译预算 |
| `program_files` | 运行沙箱需要的文件：编译产物或原始源码 |
| `run` | 程序或解释器命令、参数数组、有限环境及运行时文件需求 |

`program_files` 是文件准备规则：有编译阶段时指向成功导出的产物，没有编译阶段时指向源码。执行器统一处理可选编译、沙箱生命周期、限额、输出比较、短路和结果保存，不在这些环节按语言复制流程。

| 语言 | 源码 | 编译计划 | 用例运行计划 |
| --- | --- | --- | --- |
| `c` | `main.c` | 配置的 GCC，示例参数 `main.c -std=c17 -O2 -o program` | 在新沙箱中运行 `./program` |
| `python` | `main.py` | 无 | 在新沙箱中运行配置的 `python3 -I -B -S main.py` |

Python 首版支持单文件 Python 3 和部署提供的标准库，不提供依赖安装、第三方包选择或多文件项目上传。标准库调用同样受沙箱限制。每个用例重新启动解释器并获得源码副本，不共享解释器状态或字节码缓存。

Python 不单独预编译，语法错误在首个用例执行中形成 RE，stderr 保留有界诊断。`-I` 限制 Python 的启动环境和模块搜索路径，`-B` 禁止导入时写入 `.pyc`，`-S` 跳过自动导入 `site`；操作系统隔离仍由沙箱完成，见[Python 命令行文档](https://docs.python.org/3/using/cmdline.html)。

部署端可以修改或升级工具链。Worker 不计算摘要，不制作固定快照，不承诺同一源码在不同时间或不同 Worker 上得到相同产物。部署端修改工具链导致的变化由部署端负责。

### 7.2 基于 Isolate 默认规则的隔离策略

编译器、解释器和目标程序均在沙箱中运行。以 Isolate 官方默认隔离规则和安装配置为基线，补充任务限额、有限环境变量及纳入磁盘配额的临时目录映射，不重新搭建整套根文件系统或默认目录规则。任务请求不能放宽隔离策略。

| 范围 | 默认行为与最小调整 |
| --- | --- |
| 网络 | 保留默认独立网络命名空间，只有沙箱内 loopback；不启用 `--share-net` |
| 环境变量 | 保留默认显式传入规则，只增加配置需要的变量；不启用 `--full-env` |
| 设备 | 保留默认 `/dev` 的宿主非递归映射，不额外建立设备白名单 |
| 目录 | 保留默认 `/box` 工作区、临时 `/tmp`、只读 `/bin`、`/usr`、`/lib`、`/lib64` 及受限 `/proc`；`/dev/shm` 使用下文规定的阶段私有临时目录；不额外开放 Worker 私有目录、其他任务或答案 |
| FD | 保留默认关闭 0、1、2 以外继承 FD 的行为；Worker 接好本次受控输入输出，不启用 `--inherit-fds` |
| 进程与资源 | 保留后端受限身份与默认系统调用限制；运行阶段保留单进程限制，编译阶段显式允许必要子进程；启用控制组并传入 CPU、墙钟、内存、FD、单个文件大小及磁盘总量与 inode 配额 |

不使用 `--no-default-dirs`，不替换默认 `/dev` 映射。只有实际工具链运行缺少依赖时才补充具体路径，不提前逐项重建系统目录。默认系统目录的只读映射不限制部署端在宿主上更新工具链，也不要求制作版本快照。

`--cg` 在初始化、运行和清理时一致使用，内存限额使用 `--cg-mem` 约束整个进程树。CPU、墙钟和输出等任务预算由 Worker 显式配置，不依赖默认值。安装所需的沙箱目录、UID/GID 范围和控制组路径以官方安装配置为基线，由部署端按主机安排，见[官方默认配置模板](https://raw.githubusercontent.com/ioi/isolate/master/default.cf.in)。

编译与运行阶段均在 `--init` 时传入 `--quota=<disk_quota_kib>,<disk_quota_inodes>`，为本 box UID 设置空间和 inode 硬配额；两个配置值均为正数，空间单位为 KiB，inode 配额覆盖文件及目录等对象。部署端必须在 Isolate 沙箱根目录所在文件系统启用用户配额，保留 box UID 专用于该沙箱，并按同机 Worker 数量预留配额容量。配额设置失败属于初始化失败，本阶段不运行，见[Isolate 配额说明](https://www.ucw.cz/isolate/isolate.1.html)和 [Linux 配额单位定义](https://github.com/torvalds/linux/blob/master/include/uapi/linux/quota.h)。

`/box` 与默认临时 `/tmp` 位于上述配额文件系统；运行时用 `--dir=/dev/shm:tmp` 将默认 `/dev/shm` tmpfs 替换为同一文件系统上的新临时目录，使三处共享本 box UID 的空间和 inode 配额。额外工具链依赖只读映射。Isolate 的临时目录规则在每个阶段创建独立目录，见[目录规则实现](https://github.com/ioi/isolate/blob/master/rules.c)。达到空间或 inode 配额时，文件操作由内核以 `EDQUOT` 拒绝，编译和运行结论仍按第 5.3 节的实际执行结果形成。

Isolate 使用宿主 Linux 内核，上述策略约束可见资源和允许操作，不提供独立内核或独立物理资源。后端相关选项见[Isolate 官方手册](https://www.ucw.cz/isolate/isolate.1.html)。

## 8. 配置文件

Worker 启动时一次性读取 TOML 配置，所有门限集中配置；修改后通过重启生效，不动态重载。题目执行限额由请求明确给出，配置文件保存其允许上限和 Worker 自身预算。Scheduler 的轮询与调用超时保存在 Scheduler 配置中。

以下数值是首版配置示例，需按部署资源调整：

```toml
[server]
listen = "127.0.0.1:50051"
max_request_bytes = 16_777_216
max_response_bytes = 1_048_576

[request]
job_id_max_bytes = 128
source_max_bytes = 65_536
case_max_count = 128
input_max_bytes = 65_536
expected_output_max_bytes = 65_536
payload_max_bytes = 8_388_608

[execution]
job_wall_time_ms = 120_000
max_run_cpu_time_ms = 10_000
max_run_wall_time_ms = 15_000
max_run_memory_kib = 524_288
run_process_max_count = 1
stdout_max_bytes = 65_536
stderr_max_bytes = 65_536

[languages.c]
compiler = "/usr/bin/gcc"
compile_arguments = ["main.c", "-std=c17", "-O2", "-o", "program"]

[languages.python]
interpreter = "/usr/bin/python3"
run_arguments = ["-I", "-B", "-S", "main.py"]

[compile]
cpu_time_ms = 10_000
wall_time_ms = 20_000
memory_kib = 524_288
process_max_count = 32
diagnostic_max_bytes = 65_536
artifact_max_bytes = 16_777_216

[sandbox]
backend = "isolate"
executable = "/usr/bin/isolate"
box_id = 0
fd_max_count = 64
file_size_max_kib = 16_384
disk_quota_kib = 65_536
disk_quota_inodes = 4_096
stop_timeout_ms = 2_000
cleanup_timeout_ms = 3_000
shutdown_timeout_ms = 8_000
environment = { PATH = "/usr/bin:/bin", LANG = "C.UTF-8", HOME = "/box", TMPDIR = "/tmp" }

[results]
max_unacknowledged_results = 128
acknowledged_queue_length = 128

[logging]
level = "info"
```

Isolate box ID 由部署配置；同一宿主上不同 Worker 使用不同 ID。一个 Worker 内串行沿用这个数字，但每个阶段都重新初始化和清理沙箱，不沿用任何已执行环境。文件系统映射和权限由沙箱部署配置提供，不来自任务请求。

`sandbox.executable`、C 编译器和 Python 解释器使用绝对路径，启动时以当前用户执行各工具的 `--version` 检查可调用性；这不等同于真实沙箱权限、控制组或配额验收。配置示例见 [worker.example.toml](../config/worker.example.toml)。启动检查正值预算、CPU 不超过墙钟、运行单进程、产物大小不超过单文件和磁盘配额，以及停机预算覆盖停止与清理。传输预算为原始载荷另留协议空间：请求按用例各预留 32 字节，并加 Job ID 上限与 1 KiB；响应按编译诊断或失败 stdout/stderr 的较大载荷，加 Job ID 上限与 1 KiB。

请求合计大小按源码与全部输入、答案的字节数计算；gRPC 消息还有单独的传输上限。结果体积由编译诊断及失败用例的输出上限约束，启动时应确保响应上限能容纳这些结果。未确认结果数量有上限，已确认队列长度固定，因此内存记录不会因未获取结果持续增长。

## 9. 日志

Worker 自身的日志消息统一使用英文，以人类可读的单行文本写入 stderr，由部署端收集和轮转。日志级别从配置读取，不在 Worker 中建设日志查询服务。编译器、解释器和提交程序的原始诊断保留原文，不由 Worker 翻译。

每行包含时间、级别和事件说明；任务相关事件附带 Job ID、语言和阶段，运行事件附带用例序号。记录接纳、编译开始与结束、用例开始与结论、任务终态、停止和清理错误，并在完成时记录耗时。重复请求和频繁轮询仅在 debug 级别记录，避免淹没执行信息。

```text
2026-10-11T10:00:00+08:00 INFO  job=42 language=c phase=preparing Job accepted, cases=3
2026-10-11T10:00:00+08:00 INFO  job=42 language=c phase=compiling Compilation started
2026-10-11T10:00:01+08:00 WARN  job=42 language=c phase=running_case case=1/3 Failed to remove workspace; continuing, error="Permission denied"
2026-10-11T10:00:01+08:00 INFO  job=42 language=c phase=checking case=1/3 Case passed, elapsed=8ms
2026-10-11T10:00:01+08:00 INFO  job=42 language=c phase=checking case=2/3 Output mismatch; skipping remaining cases
2026-10-11T10:00:01+08:00 INFO  job=42 language=c phase=done Judging finished, verdict=WA, elapsed=1030ms
```

CE、WA 等正常判题结论记录为 info；未接纳、工作文件或已停止环境的清理失败记录为 warn；沙箱初始化、启动、停止和收集等执行故障记录为 error，给出操作、错误原因及可关联的 Job ID。日志不输出整份源码、完整测试数据或无界程序输出；编译和运行诊断保存在有界结果中。

## 10. 最小验证范围

验证聚焦关键业务行为和一条真实沙箱执行链，不按所有字段与错误组合建立矩阵：

- C 编译与运行、Python 直接运行：AC 不返回用例；C 的 CE 不启动运行沙箱；WA 返回首个失败用例且后续用例不执行；Python 语法错误为首个用例的 RE。确认各阶段串行、运行副本互不影响。
- 接纳与去重：忙时不登记；已有 ID 直接返回且只执行一次；同 ID 内容不比较；不支持的语言被拒绝。
- 一个代表性超限执行：能够停止进程树并调用清理；工作文件清理失败只记录警告，结果可查且继续接纳任务。
- 主动停止通过 `SIGTERM` 完成 Isolate 收尾；管理器异常退出或停止未确认时，Job 失败可查，ACK 后仍不接纳新任务。
- 使用临时调小的配额，在 `/box`、`/tmp`、`/dev/shm` 累计写入文件和创建空文件，分别验证空间及 inode 配额生效；配额设置失败时不执行程序。
- 在真实沙箱中验证宿主私有目录、继承环境和额外 FD 不可见，不能连接宿主网络。
- 结果生命周期：查询不淘汰未确认记录；确认后按定长 FIFO 淘汰；未确认区满时拒绝新任务。
- Scheduler 集成：失联后保存失败且不重新分派旧 Job，迟到结果不覆盖业务失败。

日志通过上述执行确认能够定位 Job、阶段和故障原因。只在实际缺陷或新增行为需要时补充测试，不为配置文字、日志措辞、恢复机制或边缘组合建立冗余测试。

# Worker 设计

状态：设计稿，尚未实现。

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
| Scheduler | 排队、生成 Job ID、选择 Worker、提交、轮询、保存结果并确认获取、判定失联失败 |
| Worker | 去重与接纳、选择语言计划、组织可选编译和串行运行、比较输出、保存内存记录、输出日志 |
| 语言适配器 | 根据本地配置生成固定源码名、可选编译计划和运行计划 |
| 沙箱封装 | 初始化、传入文件、施加限额、执行、收集输出和产物、停止进程树、调用清理 |
| 部署端 | 安装 C/Python 工具链和沙箱、配置权限与资源、收集日志、处理残留文件或环境问题 |

Submission 是业务提交，Job ID 是一次执行的标识。首版一个 Submission 最多对应一个 Job ID，可以直接使用唯一的 Submission ID，不要求额外建立 Job 数据表。

Job ID 由 Scheduler 分配，可以使用数据库唯一 ID 或 UUID，不由源码摘要生成。Worker 将其视为不透明字符串。相同源码的不同提交应有不同 ID；同 ID 的重复请求直接返回已知状态，即使携带的内容不同也不重新执行。

去重只覆盖本进程仍保留的记录。记录被淘汰或进程重启后，不再提供该 ID 的去重保证。Scheduler 不重新提交已经结束、失败或确认状态不明的旧 Job。以后重新判题或自动重试时使用新 ID，并可分派到任意空闲 Worker。

## 3. RPC 契约

### 3.1 服务

| RPC | 行为 |
| --- | --- |
| `SubmitJob` | 接纳任务并立即返回，不等待执行完成 |
| `GetJob` | 立即返回阶段、进度或终态，不长轮询 |
| `AcknowledgeResult` | Scheduler 保存终态后确认获取，允许该记录进入淘汰队列 |

不要求提交前探测 Worker 状态。Scheduler 根据自己的分派记录选择 Worker，最终以 `SubmitJob` 的接纳或拒绝为准。不提供容量、能力列表或接纳状态查询字段。gRPC 可以复用传输连接，业务不依赖持续在线的连接。

首版不进行调用方鉴权，不要求 token、身份 metadata 或客户端证书。所有调用方使用相同的协议校验与接纳规则。协议版本由 `acorn.worker.v1` 服务名表达，不增加请求版本字段或目标进程身份字段。

### 3.2 消息

```proto
syntax = "proto3";
package acorn.worker.v1;

service JudgeWorker {
  rpc SubmitJob(SubmitJobRequest) returns (SubmitJobReply);
  rpc GetJob(GetJobRequest) returns (GetJobReply);
  rpc AcknowledgeResult(AcknowledgeResultRequest) returns (AcknowledgeResultReply);
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
  uint64 memory_bytes = 3;
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

`FINISHED` 表示取得判题结论，包含一个 `JudgeResult`；`FAILED` 表示执行基础设施或 Job 总期限导致失败，包含 `JobFailure`。CE、WA、RE、TLE、MLE、OLE 都是判题结论，不是基础设施失败。运行中不包含终态消息。

AC 仅返回结论；CE 返回结论及有界编译诊断，不返回 `failed_case`；用例级错误返回结论及唯一的首个失败用例。输入和答案已在 Scheduler 中，不在结果中重复发送。未运行用例不生成占位记录。`completed_cases` 统计已经得到裁决的用例，包括首个失败用例；CE 时为 0。

`failed_case` 显式声明为 `optional`，用字段存在性区分“没有失败用例”和“包含失败用例”。Proto3 的单个 message 字段本身也具有存在性，见[Protobuf 字段存在性文档](https://protobuf.dev/programming-guides/field_presence/)。

### 3.3 接纳与去重

接纳过程按以下顺序执行：

1. 检查 Job ID 非空且长度在配置范围内。
2. 查询内存记录。ID 已存在时直接返回原状态及 `duplicate=true`，不比较内容，不再次执行，不延长期限。
3. 检查语言、测试用例数量、数据大小和运行限额。
4. 在同一接纳临界区确认没有执行中的 Job、未确认结果区还有空间，且不在停机中。
5. 插入 `RUNNING/PREPARING` 记录，标记 Worker 忙，启动后台执行，返回 `duplicate=false`。

步骤 2、4、5 必须保证并发 RPC 不会重复启动任务。RPC 可以同时到达，但执行器始终只有一个 Job。

忙时拒绝发生在步骤 5 之前：不插入该 Job 的记录，不启动沙箱，不将它放进内部队列。只有明确收到未接纳响应的任务，Scheduler 才能继续排队或交给其他 Worker。

| 情况 | gRPC 状态及原因 |
| --- | --- |
| 语言不是 `c` 或 `python` | `INVALID_ARGUMENT`，`UnsupportedLanguage` |
| 必填值、用例数量、大小或限额无效 | `INVALID_ARGUMENT`，`InvalidJob` |
| 正在执行或未确认结果区已满 | `RESOURCE_EXHAUSTED`，`WorkerBusy` / `ResultBufferFull` |
| 停机 | `FAILED_PRECONDITION`，`WorkerUnavailable` |
| Job ID 没有记录 | `NOT_FOUND`，`JobNotFound` |
| 确认仍在运行的任务 | `FAILED_PRECONDITION`，`ResultNotReady` |

校验内容限定为执行所需的基本条件：至少一个用例、数量不超限，源码非空，源码、单个输入、单个答案及合计字节数不超限。空输入、空答案和内容相同的多个用例均允许。时间和内存必须显式提供正值且不超过配置上限；未填值产生的 0 不表示无限额。源码是否有效交给对应的编译器或解释器判断。

### 3.4 内存记录与结果保留

Worker 使用 `HashMap<JobId, JobRecord>` 保存运行状态或终态结果，用 `VecDeque<JobId>` 保存已确认结果的淘汰顺序。这些都是进程内数据结构。

终态写入后不再修改。未确认获取的终态记录不设 TTL，也不因新任务到来被淘汰。当未确认结果数达到配置上限时，Worker 拒绝新任务，已有记录继续可查。

Scheduler 取得终态并成功保存后调用 `AcknowledgeResult`。确认将记录移入定长 FIFO 队列；超过队列长度时删除最早确认的记录。重复确认仍在队列中的记录成功返回，不重新排列顺序。记录尚未进入终态时不能确认。

`GetJob` 本身不改变保留状态。服务端发送成功不能证明 Scheduler 已读取并保存结果，因此只有显式确认才允许淘汰；确认不引入任务续租或执行恢复。gRPC 客户端和服务端可以对同一次调用得出不同的完成状态，见[gRPC 调用生命周期](https://grpc.io/docs/what-is-grpc/core-concepts/#rpc-termination)。无论正常判题还是基础设施失败，都使用相同的保留规则。

运行结束后释放源码、全部测试数据及编译产物的内存引用，尝试删除私有工作文件，只保留状态和有界结果。文件删除失败记录日志，不阻止后续任务。Worker 不持久化任务或结果，退出或崩溃后的记录保留由本协议明确排除。

## 4. Scheduler 与通信失败

Scheduler 在自己的存储中保存业务终态。提交超时或断连时，不能仅据此认为 Worker 未接纳；可以通过一次有界 `GetJob` 确认。若记录存在则继续跟踪；明确不存在则记为提交失败；无法确认则立即记为 Worker 无响应导致的失败。

正常轮询失败并被判定为 Worker 无响应后，Scheduler 立即保存本次 Job 失败，不设置恢复等待窗口，也不把该 Job 转移给其他 Worker。后台执行可能仍继续，Worker 的本地限额负责约束它。

Scheduler 的失败记录不会被迟到结果改写。如果以后再次取得旧 Job 的终态，可以丢弃其判题内容并确认获取，让 Worker 按正常保留规则回收记录。一次通信故障导致仍在执行的任务被判失败，是当前策略接受的行为。

Worker 重启后查询旧 Job 返回 `NOT_FOUND`；不接管、不恢复旧任务。Scheduler 可在后续新任务中再次选择这个地址，实际接纳仍由 `SubmitJob` 决定，不能把通信失败等同于旧任务已经停止。

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

最后一个阶段的进程树停止后，组装有界结果、调用沙箱和私有工作区清理，再写入终态并释放执行位置。清理失败不阻止释放；执行错误在发布终态前形成失败结论，不在结果发布后修改结论。结果等待获取不占用执行位置；下一 Job 可随后开始，但仍受未确认结果数量上限约束。

## 6. 统一沙箱接口

Worker 调用一次完整的阶段执行操作，输入为 `SandboxRunSpec`，输出为 `SandboxRunOutcome`。编译阶段与运行阶段共用同一接口，不向业务层暴露一套独立清理状态机。

| 输入 | 内容 |
| --- | --- |
| 程序 | 部署端配置生成的可执行文件及参数数组、工作目录、有限环境 |
| 文件 | 当前阶段所需的源码或程序副本，以及当前 stdin |
| 隔离策略 | 第 7.2 节规定的目录、设备、网络、环境和 FD 限制，不允许请求放宽 |
| 限额与收集策略 | CPU、墙钟、内存、进程数、FD、文件大小、输出及停止/清理超时；编译诊断截断保存，运行输出超限停止 |
| 导出 | 编译阶段允许导出的固定产物路径及大小上限；运行阶段无产物导出 |
| 停止信号 | Job 总期限、内部故障或进程正常停机触发的停止请求 |

| 输出 | 内容 |
| --- | --- |
| 执行结果 | 正常退出、退出码/信号，或明确的资源超限原因 |
| 收集内容 | 有界 stdout/stderr、截断标记、可用资源统计、成功编译的产物 |
| 阶段错误 | 初始化、启动、停止、收集或产物导出失败的错误信息；清理失败由封装记录日志 |

封装内部按“初始化 → 放入文件 → 执行与收集 → 停止进程树 → 导出允许内容 → 尝试清理”完成阶段。正常返回意味着进程树已经结束，输出和产物已保存，清理已经调用；不要求文件删除成功。

Isolate 的 `--run` 不包含 `--cleanup`。封装在阶段结束时调用 `--cleanup`，覆盖成功、CE、超限和内部错误；初始化成功但后续准备失败时也进入清理路径。这是调用已有清理操作，不追加资源扫描、清理报告核验或其他证明步骤，见[Isolate 官方手册](https://www.ucw.cz/isolate/isolate.1.html)。

CPU、内存及停止操作覆盖本阶段的进程树。Isolate 后端使用控制组模式，编译阶段允许编译器启动必要的子进程；运行阶段按配置限制进程数。

进程树停止后，后端清理或工作文件删除失败只输出 warn 日志，不改变 Job 结论，不退出 Worker，不暂停接纳。残留工作文件不交给后续任务，部署端负责处理；Worker 不扫描残留资源、不重试清理、不建设通用文件 GC。

下一阶段或下一 Job 仍调用正常初始化路径；初始化成功必须提供独立工作目录和本次资源计量，不能只换目录而沿用旧执行的计量状态。初始化失败则报告该次执行的基础设施错误，不能降级为使用旧环境。Isolate 的固定 box ID 可串行复用，但 `--init` 失败时不得执行程序。以上依赖初始化接口的契约，不追加清理审计或资源扫描。

进程树停止失败属于执行故障，不能按文件清理警告处理。此时报告 Job 失败并保留占用的执行位置，由部署端处置，避免在旧程序仍运行时启动下一 Job。这依赖后端执行/停止接口的完成契约，不增加独立的 PID 或 cgroup 扫描。

内部停止作用于本次进程树，用于资源超限、Job 总期限、基础设施故障和正常停机。停机时先拒绝新任务，再停止当前执行并调用相同清理路径；不提供客户端取消接口。

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

编译器、解释器和目标程序均在沙箱中运行。以 Isolate 官方默认隔离规则和安装配置为基线，仅补充任务限额和有限环境变量，不重新搭建整套根文件系统或默认目录规则。任务请求不能放宽隔离策略。

| 范围 | 默认行为与最小调整 |
| --- | --- |
| 网络 | 保留默认独立网络命名空间，只有沙箱内 loopback；不启用 `--share-net` |
| 环境变量 | 保留默认显式传入规则，只增加配置需要的变量；不启用 `--full-env` |
| 设备 | 保留默认 `/dev` 的宿主非递归映射及独立 `/dev/shm`，不额外建立设备白名单 |
| 目录 | 保留默认 `/box` 工作区、临时 `/tmp`、独立 `/dev/shm`、只读 `/bin`、`/usr`、`/lib`、`/lib64` 及受限 `/proc`；不额外开放 Worker 私有目录、其他任务或答案 |
| FD | 保留默认关闭 0、1、2 以外继承 FD 的行为；Worker 接好本次受控输入输出，不启用 `--inherit-fds` |
| 进程与资源 | 保留后端受限身份与默认系统调用限制；运行阶段保留单进程限制，编译阶段显式允许必要子进程；启用控制组并传入 CPU、墙钟、内存、FD 和文件大小限额 |

不使用 `--no-default-dirs`，不替换默认 `/dev` 映射。只有实际工具链运行缺少依赖时才补充具体路径，不提前逐项重建系统目录。默认系统目录的只读映射不限制部署端在宿主上更新工具链，也不要求制作版本快照。

`--cg` 在初始化、运行和清理时一致使用，内存限额使用 `--cg-mem` 约束整个进程树。CPU、墙钟和输出等任务预算由 Worker 显式配置，不依赖默认值。安装所需的沙箱目录、UID/GID 范围和控制组路径以官方安装配置为基线，由部署端按主机安排，见[官方默认配置模板](https://raw.githubusercontent.com/ioi/isolate/master/default.cf.in)。

Isolate 使用宿主 Linux 内核，上述策略约束可见资源和允许操作，不提供独立内核或独立物理资源。后端相关选项见[Isolate 官方手册](https://www.ucw.cz/isolate/isolate.1.html)。

## 8. 配置文件

Worker 启动时一次性读取 TOML 配置，所有门限集中配置；修改后通过重启生效，不动态重载。题目执行限额由请求明确给出，配置文件保存其允许上限和 Worker 自身预算。Scheduler 的轮询与调用超时保存在 Scheduler 配置中。

以下数值是首版配置示例，需按部署资源调整，不代表性能测量结果：

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
max_run_memory_bytes = 536_870_912
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
memory_bytes = 536_870_912
process_max_count = 32
diagnostic_max_bytes = 65_536
artifact_max_bytes = 16_777_216

[sandbox]
backend = "isolate"
box_id = 0
fd_max_count = 64
file_size_max_bytes = 16_777_216
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
- 在真实沙箱中验证宿主私有目录、继承环境和额外 FD 不可见，不能连接宿主网络。
- 结果生命周期：查询不淘汰未确认记录；确认后按定长 FIFO 淘汰；未确认区满时拒绝新任务。
- Scheduler 集成：失联后保存失败且不重新分派旧 Job，迟到结果不覆盖业务失败。

日志通过上述执行确认能够定位 Job、阶段和故障原因。只在实际缺陷或新增行为需要时补充测试，不为配置文字、日志措辞、恢复机制或边缘组合建立冗余测试。

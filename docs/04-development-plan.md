# Worker 与后端开发计划

状态：P01–P10 已完成；首版整体联调和操作验收已通过。

本计划及实施记录保留 P01–P10 基础判题交付的原范围。后续 LLM 增量的 L01–L04 已完成数据契约与结构版本 3、客户端与策略、独立队列与生命周期、手动 HTTP 与查询；GUI 和最终联调待 L05–L06。任务与验收见[第 08 篇 L01–L06](08-llm-code-review-design.md#10-实施顺序与有限验收)，相关数据、后端与前端合同同步维护在各设计文档。本文的“现有接口”“结构”和检查结果按各阶段原范围理解，不表示后续结构版本 3 或完整审查功能由原任务交付；文档维护边界见[索引](README.md)。

## 1. 目标与依据

本计划将现有设计拆为可依次完成的开发任务。执行契约以 [Worker 设计](01-worker-design.md)为准，持久化以[数据库设计](02-database-design.md)和[初始 SQL](../sql/001-initial-schema.sql)为准，后端职责与 HTTP 契约以[后端设计](03-backend-design.md)为准。

首版交付两个程序：独立 Worker，以及内含 Scheduler 的后端。完成后，可以使用 HTTP 提交 C 或 Python 单文件源码，经 gRPC 判题后查询持久化结果。每个 Worker 同时执行一个 Job，Job 内编译、运行和各用例始终串行；不同 Worker 可以处理不同 Submission。

当前已有设计、建表 SQL、Cargo workspace、共享 Proto、两个程序入口，以及可独立启动的 Worker：配置、日志、语言计划、Isolate 阶段接口、串行判题、RPC 和结果保留均已实现。后端已完成配置、日志、数据库访问、开发用户预置、Scheduler、HTTP API，以及 P09 的启动装入、旧在途记录中断、正常停机和故障退出。P10 已完成源码样例、真实单／双 Worker 联调和独立程序从空库启动及重启查询，首版十个步骤均有实际产物与验证记录。

运行时检查限于已约定的请求、资源、并发接纳和业务状态要求。实现依赖组件的正常返回与错误报告，不增加跨域检查、Worker 身份核验或额外清理审计。

## 2. 工程与交付物

采用一个 Cargo workspace，包含三个 crate：

```text
Cargo.toml
Cargo.lock
proto/acorn/worker/v1/worker.proto
crates/
  protocol/                 # acorn-protocol：生成并导出 Proto 类型与服务代码
    build.rs
    src/lib.rs
  worker/                   # acorn-worker：独立 Worker 程序及内部模块
    src/
      main.rs
      lib.rs
      config.rs
      logging.rs
      language/
      sandbox/
      executor.rs
      jobs.rs
      rpc.rs
  backend/                  # acorn-backend：HTTP、Scheduler 与数据库访问
    src/
      main.rs
      lib.rs
      config.rs
      logging.rs
      http/
      problems.rs
      submissions.rs
      scheduler/
        mod.rs
        worker_client.rs
      database/
config/
  worker.example.toml
  backend.example.toml
examples/
  seed-user.sql             # 已有开发用测试用户预置文件
  submissions/
sql/001-initial-schema.sql   # 已有建表脚本
sql/002-example-problem.sql  # 已有两数求和题，随正式发布保留
```

`protocol` 只共享通信契约，生成代码写入构建输出目录。HTTP 请求与响应结构归后端，沙箱和语言计划归 Worker。验证代码放在对应 crate 的单元测试或 `tests/` 下。

`examples/` 放开发用测试用户预置文件，以及少量 C/Python 提交源码样例，供手动联调和主要路线验证。正式保留的两数求和题及测试数据放在 [002-example-problem.sql](../sql/002-example-problem.sql)，由部署端在新库初始化时装入。

两个程序均通过 `--config <path>` 指定 TOML 文件。配置示例给出可用于本地联调的明确值；实际监听地址、工具路径、Isolate box ID 和门限由部署配置提供。依赖版本在建立工程时选择并锁入 `Cargo.lock`。

## 3. 开发顺序

| 步骤 | 依赖 | 主要产物 | 完成标志 |
| --- | --- | --- | --- |
| P01 工程骨架与协议 | 无 | workspace、Proto 生成、两个程序入口 | 全部 crate 可编译，客户端与服务端来自同一协议 |
| P02 Worker 配置与语言计划 | P01 | TOML、英文日志、C/Python 适配器 | 能生成正确的编译与运行计划 |
| P03 Isolate 沙箱封装 | P02 | 完整阶段执行接口 | 真实执行、收集、停止和清理可用 |
| P04 串行判题执行器 | P03 | 可选编译、用例循环、比较与结果 | C/Python 判题可在内部调用中完成 |
| P05 Worker RPC 与结果保留 | P04 | 三个 RPC、内存记录、ACK 淘汰 | Worker 能独立对外提供完整服务 |
| P06 后端数据库访问 | P01 | SQLx、业务查询与写入、预置样例 | 可保存和查询符合契约的提交 |
| P07 Scheduler | P05、P06 | 有界队列、节点缓存、分派与轮询 | 内存任务可经真实 Worker 取得并保存结果 |
| P08 HTTP API | P07 | 题目查询、创建提交、历史与结果查询 | HTTP 到数据库和 Worker 的闭环可运行 |
| P09 启动、停机与故障边界 | P05、P08 | 启动装入、停止接纳、后台监督 | 重启和正常停机符合既定规则 |
| P10 联调与使用说明 | P09 | 主要路线验证、启动与操作说明 | 按文档可完成一次实际判题与查询 |

P05 是 Worker 独立交付点；P08 是后端正常业务闭环交付点；P10 是首版整体交付点。P06 仅依赖公共工程，可以在后端开发开始时单独完成。

## 4. 具体开发任务

### 4.1 P01：建立工程骨架与共享协议

1. 创建 workspace 和三个 crate，配置两个程序入口及共享协议依赖。
2. 将 Worker 设计第 3.2 节的 Proto 落到 `proto/acorn/worker/v1/worker.proto`，保留三个 unary RPC、终态 `oneof` 和可选失败用例、统计字段。
3. 在 `protocol/build.rs` 接入 Tonic/Prost 代码生成，Worker 使用生成的服务端，后端使用生成的客户端。
4. 为两个入口接入配置路径参数，先保证模块能组装并编译。具体配置解析和业务逻辑在后续步骤补齐。

**完成条件：** workspace 可编译；Proto 修改后可重新生成，生成文件无需手工编辑。

### 4.2 P02：实现 Worker 配置、日志与语言适配器

1. 按 Worker 设计第 8 节定义配置结构和示例文件，读取监听地址、请求门限、执行预算、编译预算、沙箱参数及结果保留上限。
2. 启动时校验正值门限及相互关系，例如结果消息上限应容纳有界诊断和失败用例输出；检查 Isolate、GCC、Python 3 的配置路径及必要工具是否可用。
3. 初始化 tracing 文本日志，英文消息输出到 stderr；任务相关日志附带 Job ID、语言、阶段及用例序号。
4. 实现 `language_id -> LanguagePlan`：C 使用 `main.c`，生成一次编译和 `./program` 运行计划；Python 使用 `main.py`，没有编译阶段，生成 `python3 -I -B -S main.py` 计划。
5. 计划只包含文件准备规则、命令参数数组、环境和预算。由共同执行器解释计划，用户请求不能提供命令或挂载参数。

**完成条件：** 合法示例可加载，两种语言计划可生成；无效启动配置明确报错。配置和日志随功能验证，不逐字段构造测试矩阵。

### 4.3 P03：实现完整的 Isolate 阶段执行接口

1. 定义输入 `SandboxRunSpec`：本阶段要放入的文件与 stdin、命令参数数组、资源限额，以及编译成功时允许导出的产物路径。定义输出 `SandboxRunOutcome`：有界 stdout/stderr、退出或超限原因、可用资源统计、可选的编译产物，以及能够区分停止未确认的阶段错误。
2. 实现 Isolate 后端的共同流程：初始化新环境、放入文件、执行与收集、停止本阶段进程树、导出允许内容、调用清理。初始化、运行、清理这三次 Isolate 调用均传入 `--cg` 和同一 box ID，使用一致的控制组模式；每次初始化同时设置空间和 inode 硬配额。
3. 按 Worker 设计第 7.2 节保留 Isolate 默认设备、网络、环境和 FD 规则，补充部署配置中的有限环境及限额；将 `/dev/shm` 映射到纳入同一磁盘配额的阶段私有临时目录。开发机安装、用户配额与权限由部署准备，Worker 以启动用户运行。
4. 接入 CPU、墙钟、进程树内存、进程数、FD、单个文件大小及磁盘总量与 inode 配额。编译阶段允许必要子进程，运行阶段保持既定进程数限制；解析沙箱实际报告的退出与超限原因，不凭 SIGKILL 推断 MLE。配额设置失败时不执行本阶段。
5. 持续收集 stdout/stderr：编译诊断有界保存、超出后继续排空；运行输出超过上限后停止程序并形成 OLE。缺失统计保持缺失。
6. 编译产物只从固定路径导出，检查常规文件、链接和大小；运行文件使用独立副本。执行进程尚未启动或进程树已确认停止时，清理覆盖正常、编译失败、超限及内部错误路径；停止未确认时跳过产物导出和依赖停止的清理。
7. 区分停止失败或停止未确认与已停止环境的文件清理失败：前者返回可分类的执行故障并保留执行占用，后者记 warn 后继续。执行器依据阶段结果处理，不增加清理扫描或恢复流程。

Worker 以子进程启动 `isolate --run`，通过 `Child::wait().await` 等待管理器退出，同时持续收集输出。Isolate 以退出码 0 或 1 完成执行时，依据其收尾契约确认本阶段进程树已停止，再处理元数据、导出产物并调用清理。需要主动停止时，向管理器发送 `SIGTERM`，并在配置的停止超时内等待退出；原始停止原因由 Worker 保存。管理器被信号终止、等待失败、停止超时或无法确认停止时，返回明确的停止未确认错误，由 P05 发布 Job 失败并保留执行位置。阶段输入与输出的完整字段见 Worker 设计第 6 节。

**完成条件：** 在配置好的 Linux/Isolate 环境中实际执行简单编译、程序和 Python 命令；输出及退出原因可用，确认停止后调用清理。用一个代表性超限验证停止路径，可控的管理器异常退出返回停止未确认。用临时调小的配额验证三个可写目录共享空间和 inode 上限，配额设置失败时不执行程序；实际确认宿主私有目录、继承环境、额外 FD 和宿主网络的隔离。

### 4.4 P04：实现串行判题执行器

1. 建立服务端生成名称的 Job 私有工作区，保存源码及执行上下文；工作目录名不直接拼接 Job ID。
2. 执行语言计划的可选编译阶段：C 编译一次并导出产物；编译用户错误或编译超限形成 CE，基础设施错误形成 Job 失败。Python 直接使用原始源码。
3. 按请求顺序循环用例。每次初始化新运行沙箱，复制原始程序文件，仅传入当前输入；前一阶段进程树停止并完成收尾调用后才开始下一阶段。
4. 保存当前输出、退出信息及可用统计，结束沙箱后形成运行结论；正常退出时，删除 stdout 和答案整个字节串末尾的 ASCII 空白后比较。
5. 当前用例 AC 才执行下一例；WA、RE、TLE、MLE、OLE 立即短路。按既定优先级处理已确认超限，返回唯一首个失败用例。
6. 接入从任务接纳时刻计算的 Job 总期限和内部停止信号。AC、CE、首例失败或执行错误均进入同一段 Job 收尾：执行进程尚未启动或进程树已确认停止时，尝试删除私有工作文件，释放源码、全部用例及编译产物的内存引用，只保留结论、首个失败用例或编译诊断等大小受配置限制的结果。停止未确认时保留可能仍在使用的资源，将该错误交给 P05 发布失败并保留执行位置。各阶段沙箱的清理由 P03 封装负责。

**完成条件：** C 的 AC、CE 和 WA，以及 Python 的 AC、首例语法错误 RE 能正确返回；CE 不启动用例，失败用例之后不再执行。不同用例使用独立程序副本，未知统计不被补成 0。执行器可以由测试直接调用，无需先经 HTTP。

### 4.5 P05：实现 Worker RPC、接纳与内存结果

1. 建立 Job 哈希表、单个执行占用和已确认结果 FIFO。运行中记录进度，终态记录只保留有界结果。
2. 实现 `SubmitJob`：先检查 ID 并按 ID 查重，再检查新任务语言、用例、大小和限额；登记前在同一接纳临界区复查 ID，检查忙、停机和结果区空间，接纳后登记并启动后台执行。
3. 将接纳时刻或对应总期限传入 P04。提交 RPC 立即返回，后台执行不绑定提交连接的生命周期；同 ID 请求直接返回原记录，不比较内容。
4. 实现 `GetJob` 的运行状态与终态转换，以及 `AcknowledgeResult`。查询不淘汰记录；ACK 后进入定长 FIFO，重复 ACK 不调整顺序。
5. 绑定既定 gRPC 状态和原因，配置发送与接收消息上限。忙时拒绝发生在登记之前，Worker 内不排队。
6. 执行进程尚未启动或进程树已确认停止时，终态发布前完成执行收尾，发布终态后释放执行位置，未确认结果仍保留。停止失败或停止未确认时仍发布可查询的失败终态，但保留执行位置、拒绝新任务；结果获取、ACK 或结果记录淘汰不释放该位置。将此情况与普通清理警告分开处理。

**完成条件：** 真实 gRPC 客户端可提交 C/Python、轮询并 ACK；已有 ID 只执行一次，忙时没有新记录，结果区满时拒绝新任务，ACK 后按 FIFO 淘汰。停止未确认时失败可查，ACK 后仍不接纳新任务。此阶段交付可独立启动的 Worker。

### 4.6 P06：实现后端数据库模块与预置数据

1. 接入后端 TOML 与英文日志，配置数据库路径、预置测试用户和 HTTP/Scheduler 门限。
2. 建立共用现有 Tokio runtime 的 SQLx `SqlitePool`，固定最多一个连接；配置外键和有限 busy timeout，检查已有数据库结构版本。
3. 实现业务所需的明确查询：题目列表、最新与指定版本、版本语言限额、有序用例、幂等键查询、提交列表与详情、启动排队读取。
4. 实现短事务写入：固定最新版本并创建提交、条件更新业务状态、保存不可覆盖的终态、将启动时旧在途记录标记为 `SchedulerInterrupted`。源码与测试数据按 BLOB 绑定。
5. 定义终态详情类型，按数据库契约处理 AC、CE、首个失败用例及执行失败；字节字段使用 Base64，`verdict` 独立写列。
6. 提供开发用 `examples/seed-user.sql` 插入一个测试用户，密码字段保存预置的合法 Argon2id 编码哈希，当前仅用于满足账号记录格式。示例题复用已有的 `sql/002-example-problem.sql`：先创建当前版本指针为空的题目，再写版本、C/Python 限额和连续用例，最后在同一事务中更新发布指针。题目数据随正式发布保留。

**完成条件：** 使用现有建表脚本初始化临时数据库后，可装入示例题目、创建并查询提交、保存主要结果。用户、版本、语言引用有效，同键只创建一条记录，条件更新不能覆盖已有终态。

### 4.7 P07：实现 Scheduler 队列、分派、轮询与 ACK

1. 定义完整任务数据和进程内 Scheduler 句柄，用 Tokio `Semaphore` 实现待接纳名额：创建前尝试取得一个 `OwnedSemaphorePermit`，无名额立即返回 QueueFull；入队时把 permit 随任务转交，等待接纳期间继续持有。明确接纳或本地失败时释放，忙时退回队列仍持有；回滚、查重竞争或合并投递释放多余 permit。以此统一计数，不另设重复的容量计数器。
2. 实现 FIFO 队列、轮转选择和节点缓存：可尝试、本地占用、暂缓尝试；记录当前 Job ID、目标地址和再次尝试时间。节点状态不写库，也不增加状态探测 RPC。
3. 在 `scheduler::worker_client` 中封装三个 Tonic RPC、连接复用、各自 deadline、消息大小限制和传输错误分类。
4. 分派前条件提交 `queued → dispatching`，更新未生效时丢弃过期投递并释放本地资源；成功后才向选定 Worker 提交。明确接纳后更新为 `running`，后续查询和 ACK 始终使用同一地址、同一 Job ID。
5. 忙、结果区满或停机且明确未接纳时，写回 `queued`、退回队首并暂缓该节点；立即选择其他候选 Worker。如果不存在其它空闲节点，则等待。调度循环管理队列，在途 RPC 跟踪使用异步任务并回送完成事件，避免等待一个 Worker 时阻塞其他节点分派。
6. 提交超时或断连只做一次有界查询；有记录则继续跟踪或保存终态，不存在或无法确认则失败。正常轮询失败立即保存失败，结束跟踪，不把旧 Job 重新交给其他 Worker。
7. 校验终态形状后条件持久化，等待保存状态、结论、详情和终态时间的数据库事务提交成功，再调用 `AcknowledgeResult`，允许 Worker 淘汰该结果。ACK 失败只记日志；迟到结果不能覆盖失败。跟踪结束归还尚未释放的名额、清除路由并更新缓存。

**完成条件：** 将一个已入库的排队任务交给 Scheduler 后，可通过真实 Worker 取得并保存结果、随后 ACK。可控的忙响应能够使任务立即改派另一候选节点；两个节点能处理不同提交。用可控 RPC 返回验证失联不重交及保存先于 ACK，不依赖罕见网络时序。

### 4.8 P08：实现 HTTP 服务与本地交接

1. 按后端设计第 5 节实现题目列表、最新详情、显式版本详情，以及提交创建、列表和详情六个路由。
2. 定义 Serde 请求与响应结构、分页筛选及英文错误响应；将框架解析与路由错误映射到既定 HTTP 错误码表。接入请求体限制和请求日志。
3. 创建接口从 `Idempotency-Key` 读取 UUID，用配置测试用户确定归属，按 UTF-8 编码源码。先查同键记录，按题号、语言和源码比较内容；同内容保留原 ID 和固定版本，仅在仍为 queued 且不在内存时重新交接。
4. 新建前预留 Scheduler 名额，在短写事务中再次查重，仅新提交选取最新版本并装配任务；提交后归还数据库连接，将完整任务和预留名额交给 Scheduler。回滚、查重竞争或交接合并时释放多余预留，已有排队记录重新交接也遵守队列上限。Worker 空闲与执行完成不影响创建响应时机。
5. 让创建服务持有事务与入队操作的生命周期，HTTP 处理器只等待结果；客户端断开仍完成已经开始的短创建操作，正常停机等待其结束。
6. 落实新建 `202`、同键同内容 `200`、冲突 `409`、队列满 `503` 等契约。查询只读持久化状态，默认题目查最新版本，历史提交固定旧版本。

**完成条件：** HTTP 提交 C 和 Python 后，轮询详情可获得终态；同键重传返回同一 Submission，冲突明确报错。队列满时不新建记录，已知提交仍可查询；响应中的详情、Base64 和缺省字段符合契约。

### 4.9 P09：补齐启动、停机与故障处理

1. 后端在开放 HTTP 前完成配置、数据库版本和测试用户检查，将旧 `dispatching/running` 保存失败，再按顺序装入 `queued`；数量超过上限时明确退出，日志中给出配置修改建议。
2. 启动装入使用提交的固定版本数据；此后正常调度只读内存队列，不联系旧任务的 Worker，也不补发旧 ACK。
3. 后端正常停机停止接纳新提交与新分派，等待短创建操作、在途结果保存和一次有 deadline 的 ACK；未分派提交保持 `queued`。
4. Worker 正常停机先拒绝新任务，再通过 P03/P04 的内部停止和共同清理路径结束当前执行；不增加远程取消接口。
5. 程序入口监督 Scheduler 后台任务，区分数据库错误边界：运行期普通只读操作失败记录英文错误并返回 `500 InternalError`，后端继续运行；关键写入失败，以及读取或写入中明确的磁盘满、I/O 故障和数据库损坏等严重故障触发退出。创建写事务开始前的幂等查询按普通只读处理，写事务及状态、终态和启动中断写入按关键写入处理，适用的锁竞争先经过有限等待。
6. 致命故障立即停止接纳与新分派，等待已开始的短创建操作完成事务处理及收尾后，结束其余本地任务跟踪并尽快退出，不等待其他 Worker 完成。没有成功保存的结果不 ACK，也不声称已持久化失败。

**完成条件：** 使用预置的 queued 与在途记录验证重启处理；正常停机后排队记录仍可在下次装入。可控的普通只读失败返回 `500` 且后端继续运行；关键写入、严重存储故障和后台任务错误按既定边界尽快退出，未保存结果不 ACK，也不进入自动修复流程。

### 4.10 P10：完成联调、样例与操作说明

P10 是首版整体交付验收阶段。交付范围仍为已有的 C/Python 单文件判题闭环；开发工作集中在可复用样例、真实联调、操作说明及联调中发现的具体缺陷修正。

#### 4.10.1 验收范围

验收覆盖五份源码样例、真实单 Worker 判题、双 Worker 并行与忙时换节点，以及按说明启动独立程序、提交、查询和重启后读取结果。复用 P09 的启动停机与故障边界检查，仅对联调发现的问题补充回归。实际结果见[第 7.12 节](#712-p10已完成)。

#### 4.10.2 实施顺序与产物

1. **先补源码样例。** 在 `examples/submissions/` 放置以下五个文件，均针对正式保留的“两数求和”题目版本 1。样例及期望结论写入项目说明，真实 HTTP 验收从这些文件读取源码，避免维护第二套内嵌样例。

   | 文件 | 预期持久化结果 | 核对重点 |
   | --- | --- | --- |
   | `a-plus-b.c` | `finished / AC` | 编译一次，四个用例全部通过 |
   | `a-plus-b.py` | `finished / AC` | Python 直接运行，四个用例全部通过 |
   | `compile-error.c` | `finished / CE` | 有编译诊断，不进入运行用例 |
   | `wrong-answer.py` | `finished / WA` | 第一个用例失败，保存失败用例与有界输出 |
   | `time-limit.c` | `finished / TLE` | 使用持续计算触发题目的现有时限，在首个失败用例短路 |

2. **完善单 Worker 真实验收。** 扩展现有 `real_http_worker_acceptance`，复用上述样例增加一个代表性 TLE。通过 HTTP 创建和轮询终态，核对数据库状态、结论及 CE/WA/TLE 详情；沿用结果保留上限为 1 的设置，验证持久化之后 ACK、后续任务仍可接纳及旧结果淘汰。判题结论 CE/WA/TLE 均为 `finished`，不能误记为执行故障 `failed`。
3. **补一项双 Worker 真实验收。** 复用现有测试夹具和生成的 gRPC 客户端，使用真实 Isolate 启动两个不同监听地址、不同专用 box ID 的 Worker。先通过直接 gRPC 请求使一个 Worker 被后端未知的任务占用，再经 HTTP 提交，确认该节点明确返回忙后，同一提交立即交给另一可用候选节点，不等待下一轮定时重试。另验证两个 HTTP 提交可同时由不同 Worker 执行，各自 Job 内仍串行；终态分别保存并 ACK。使用有预算上限的任务和有界等待，通过运行状态、节点及 Job ID 判断接纳和重叠，不以毫秒级耗时阈值作为通过条件。
4. **补齐独立程序操作步骤并实际照做。** 在现有 README 中串起构建、环境准备、空库初始化、用户和题目预置、配置修改、Worker 先于后端启动、从样例文件构造 HTTP JSON、生成幂等键、按 `Location` 轮询、读取历史及停止程序。说明重复提交使用原键、主动新提交使用新键，列出双 Worker 的地址和 box ID 配置方式。沿用正式 SQL 和现有配置文件，核对两端请求、结果和资源门限；系统配额、控制组和 Isolate 权限仍由部署端准备。用实际 `acorn-worker`、`acorn-backend` 程序，从新数据库完成 C/Python 各一次 AC，并在后端正常重启后查询原持久化结果。
5. **核对日志并记录验收。** 用同一个 Submission/Job ID 在现有英文日志中对应创建、Worker 地址选择、接纳、执行和终态保存；确认失败样例有明确操作及原因。仅在发现缺失时补必要字段或日志。在本文件的实施记录中追加实际构建和验证命令、必要环境条件、结果及未执行项。

#### 4.10.3 完成条件与验证边界

- 五个样例可重复提交，真实单 Worker 验收得到约定的 AC/CE/WA/TLE 和失败详情，结果持久化与 ACK 顺序保持正确。
- 真实双 Worker 验收证明明确忙拒绝后的立即换节点，以及不同提交同时执行；不把受控 Worker 的结果计为该项通过。
- 按 README 在已准备好的开发机上，从空数据库启动两个独立程序，完成 C/Python 提交、轮询、历史查询、正常停止及重启后结果查询。
- 修改 Rust 时执行格式检查、workspace 编译、Clippy 和相关测试；真实验收使用明确的专用 box ID 单独运行。仅修改文档时检查格式和本地链接，按第 5 节选择验证范围。
- 每项记录实际结果。受环境限制未运行的真实验收标注为未执行，不据此宣称对应交付检查通过。联调不扩展为穷举错误／资源矩阵、压力测试或部署自动化框架；第 7.8 节的期限测试问题仍需单独定位与修正。

## 5. 验证与提交方式

每个步骤完成时，必须运行 Rust 格式检查、Clippy 和受影响 crate 的编译检查，并运行与本次改动有关的测试。基础命令为：

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p acorn-worker --locked
cargo test -p acorn-backend --locked
```

上述命令在 P01 建立工程后使用；Worker 与后端测试按改动范围选择。真实沙箱验证单独标明 Isolate 和权限前提，不能把测试替身的结果当作真实隔离验证。环境不具备时如实记录未运行。

文档 lint 在 workspace 中统一设为 `deny`，所有 crate 必须继承：公共 API 使用 `missing_docs`，私有项补充使用 `clippy::missing_docs_in_private_items`。自写模块、结构体、字段、方法和函数缺少 Rustdoc 文档时检查失败；生成代码只在包含输出的专用模块局部豁免。检查范围与配置说明见[项目说明](../README.md#构建与检查)。

测试重点是正常业务闭环、接纳与幂等、失败短路、结果保存与 ACK、明确拒绝后的重新分派和既定启动停机规则。失败分支通过可控结果验证，不刻意制造小概率故障、严格时序窗口或庞大的字段组合矩阵。仅在发现实际问题或新增行为时补回归覆盖。

开发按步骤形成可审查的小批改动；每批说明实际产物和完成条件，更新受影响的设计、协议与使用说明。Git 提交仍由用户明确要求时执行。

## 6. 首版交付检查

- Worker 可独立启动，通过三个 RPC 完成 C/Python 串行判题、结果获取及确认，真实沙箱隔离与收尾已验证。
- 后端可通过 HTTP 查询题目、创建提交、查询历史与结果；Scheduler 队列、节点缓存及状态转换符合设计。
- 单库保存历史和唯一终态，幂等键生效，历史版本引用正确；保存结果早于 ACK。
- 启动、正常停机和规定的失败处理可复现；配置、样例和英文日志足以支持本地运行与排查。

## 7. 实施记录

### 7.1 P01：已完成

已建立包含 `acorn-protocol`、`acorn-worker`、`acorn-backend` 的 Cargo workspace，采用 Rust 2024 edition。依赖由 `Cargo.lock` 锁定，本次使用 Tonic / tonic-prost / tonic-prost-build 0.14.6、Prost 0.14.4、Clap 4.6.7。

[worker.proto](../proto/acorn/worker/v1/worker.proto) 与 Worker 设计第 3.2 节的 Proto 一致，包含三个 unary RPC、终态和退出信息的 `oneof`、可选失败用例及统计字段。[构建脚本](../crates/protocol/build.rs) 将消息和客户端、服务端生成到 Cargo 的 `OUT_DIR`，通过 `acorn_protocol::worker::v1` 导出；Worker 的 `rpc` 模块引用服务端接口，后端的 `scheduler::worker_client` 引用客户端。

两个程序入口均支持必填的 `--config <PATH>`、`--help` 和 `--version`。当前传入路径只完成参数解析，随后明确提示服务尚未实现并以状态码 1 退出；不读取 TOML 或启动服务。配置解析分别在 P02、P06 补齐。构建与入口说明见[项目说明](../README.md)。

本次验证环境为 Rust/Cargo 1.101.0-nightly、protoc 3.21.12。已执行：

- `cargo fmt --all -- --check`：通过。
- `cargo check --workspace` 及锁定依赖后的 `cargo check --workspace --locked`：三个 crate 编译通过。
- `cargo test --workspace --locked`：两个协议测试通过，覆盖可选统计的零值与缺失值、零退出码，以及执行失败独立的终态分支。Worker 和后端尚无业务测试。
- 分别通过 `cargo run --quiet --locked -p <package> -- <args>` 检查两个程序：帮助和版本返回 0；缺少配置路径返回 2；包含空格的配置路径正确传入，服务未实现提示返回 1。
- 比对 Proto 与设计中的代码块，内容一致。临时在 Proto 的消息前加入注释，重新编译后确认生成代码出现该注释；恢复原文件后再次编译，确认生成代码也恢复。

首次增量生成验证发现，位于 crate 目录之外的 Proto 修改没有触发重新生成。构建脚本现已显式输出该文件的 `rerun-if-changed`，并跟踪 `PROTOC` 环境变量；上述修改与恢复检查在修正后通过。首次依赖下载受沙箱内 crates.io DNS 解析限制，获准访问网络后完成下载和编译。

本阶段尚未执行真实 gRPC 判题或沙箱验证，不能据此宣称 Worker 服务可用。下一步按 P02 实现 Worker TOML 配置与校验、英文日志及 C/Python 语言计划。

### 7.2 文档约束与 Clippy：已完成

workspace 已启用 `missing_docs = "deny"` 和 `clippy::missing_docs_in_private_items = "deny"`，三个 crate 均继承规则。补齐现有自写源码的模块、结构体及入口文档；Tonic/Prost 输出仅在 `worker::v1` 模块局部豁免，未修改生成文件。项目说明、本计划及 `AGENTS.md` 均将 Clippy 列为 Rust 代码变更的必备检查。

已用临时缺文档代码验证公共模块、结构体、字段、方法、函数，以及私有模块、结构体、字段和方法都会使 Clippy 失败，随后恢复原源码。最终 `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 和 `cargo test --workspace --locked` 均通过，两个协议测试通过。此变更不扩展 P01 的运行能力，下一开发步骤仍为 P02。

### 7.3 P02：已完成

已提供 Worker TOML 配置类型与 [worker.example.toml](../config/worker.example.toml)，校验门限和预算关系，并以启动用户检查 Isolate、GCC、Python 的 `--version`。日志使用 tracing 单行英文文本、时间戳和配置级别，输出到 stderr，不继承环境日志级别。C/Python 适配器只生成命令数组、有限环境、编译预算和文件规则；C 编译一次得到固定 `program`，Python 无预编译，使用 `-I -B -S main.py`。

已运行 `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test -p acorn-worker --locked`。三个 Worker 测试通过，覆盖示例语言计划、代表性的无效预算和缺失工具。有效配置的入口检查确认英文日志写入 stderr；入口随后明确提示 RPC 尚未实现并返回 1，未宣称服务启动。下一步 P03 验证真实沙箱接口。

### 7.4 P03：已完成

已实现 [Isolate 阶段接口](../crates/worker/src/sandbox/mod.rs)：初始化时设置空间及 inode 配额，复制阶段文件，以默认隔离规则、有限环境和显式资源预算执行；`/dev/shm` 映射为配额文件系统上的私有临时目录。异步持续收集 stdout/stderr，编译诊断共用有界保留预算，运行输出超限触发 SIGTERM。管理器退出码 0/1 确认停止后才解析元数据、独立导出固定常规产物并调用清理。清理失败记 warn；停止未确认返回独立错误并保留资源和占用。Job 期限和内部停止请求均通过同一停止路径处理。

已执行 `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 和 `cargo test -p acorn-worker --locked`。九个常规 Worker 测试通过，覆盖原 P02 三项，以及元数据缺失与 SIGKILL 不推断 OOM、固定产物导出边界、有界编译诊断与清理警告、配额初始化失败不执行、异常管理器保留占用和内部停机停止。默认命令跳过需要宿主权限的真实测试。

真实验收使用 Isolate 2.6、普通用户、专用 box、cgroup v2，以及启用用户配额的 ext4 沙箱文件系统。管理员允许普通用户初始化后，实际完成带配额初始化和清理。真实验收在宿主执行环境运行。

通过[项目说明中的命令](../README.md#isolate-阶段验收)单独执行一个真实验收测试，结果通过，实际覆盖：

- C 编译并导出程序，两次使用独立程序副本计算得到 `42`；Python 命令正常执行。取得实际 CPU/墙钟统计，阶段结束后调用清理。
- Python 持续输出触发 OLE，保留 4096 字节 stdout 和截断标记，原始停止原因保持为输出超限；确认停止后清理。
- 分别使用 512 KiB 空间配额和 12 inode 配额，在 `/box`、`/tmp`、`/dev/shm` 累计写入，三处均可写且累计达到同一配额时得到 `EDQUOT`。
- 沙箱不能访问实际存在的宿主私有文件，不继承宿主测试环境变量和额外可继承 FD，也不能连接同时存在的宿主 loopback 监听端口。

管理器异常退出和配额设置失败由测试替身触发，只验证对应故障处理。尚未实现 Job 判题结论、失败短路或对外 RPC；下一步 P04 使用此接口实现串行判题执行器。

### 7.5 P04：已完成

已实现 [`Executor`](../crates/worker/src/executor.rs)：共用语言计划与完整阶段接口，C 只编译一次、Python 直接运行，各用例复制原始程序文件并串行执行；按固定尾部六种 ASCII 空白规则比较，首个失败立即短路。返回 AC、CE 或唯一首个失败用例，保留真实可选统计；已确认超限按 MLE、OLE、TLE 选择。阶段与完成数通过同步回调提供给后续 RPC 层。

Job 总期限和内部停止沿用 P03 的停止通道。普通结束在返回终态前尝试删除私有文件并释放请求和产物；停止未确认保留 Job 上下文和执行占用。失败诊断限制为 512 个 UTF-8 字节，代码与职责见 Worker 设计第 5.3 节。

已执行 `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 和 `cargo test -p acorn-worker --locked`。十五个常规 Worker 测试通过，其中新增六个执行器测试覆盖 C AC/CE/WA、编译诊断截断、Python 独立源码副本与语法错误 RE、首个失败短路、精确比较、总期限、超限优先级及 OLE 有界结果。测试管理器调用真实 GCC/Python，但不提供隔离，这些结果只证明执行编排。

使用专用 box 201 在宿主执行环境单独运行 `ACORN_ISOLATE_BOX_ID=201 cargo test -p acorn-worker --locked --test executor -- --ignored --nocapture`，一个真实验收测试通过，覆盖 C AC/CE/WA、Python AC/RE 及完成用例数。工具沙箱内同一命令先因 Isolate 初始化返回 `Must be started as root` 失败，随后在宿主权限环境通过；未把工具沙箱失败记作宿主部署故障，也未重新宣称 P03 全部隔离项目再次验收。RPC 尚未实现，下一步 P05。

### 7.6 P05：已完成

已实现 [`WorkerService`](../crates/worker/src/rpc.rs)、[内存记录与 ACK 队列](../crates/worker/src/jobs.rs)，并将 [Worker 入口](../crates/worker/src/main.rs) 接入实际 Tonic 监听。三个 unary RPC 使用既定状态与原因；配置传输上限，校验新请求的语言、用例、字节数及限额。ID 查重先于内容校验，登记前在同一接纳临界区复查；接纳即返回，后台执行不绑定提交连接。运行进度保存在内存，终态在收尾后发布，记录仅含有界查询结果。

查询不改变结果保留；未确认区满时拒绝新任务，ACK 后按定长 FIFO 淘汰，重复 ACK 不调整顺序。执行占用与结果记录分开，停止未确认时失败可查，ACK 或记录淘汰均不释放位置。Ctrl-C/SIGTERM 停止新接纳并沿 P03/P04 通道停止当前执行、等待收尾；超时或仍未确认停止返回错误，不 abort 执行 future 或宣称成功。后端入口仍为骨架。

已执行 `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`，均通过。在宿主环境执行 `cargo test -p acorn-worker --locked --test rpc`，新增六个常规测试通过；随后 `cargo test --workspace --locked` 的二十三个常规测试通过（Worker 二十一个、协议两个），三个真实验收测试默认忽略。覆盖八个并发同 ID 请求只执行一次、无效内容的重复请求、忙时不登记、连接更换后继续执行、结果区满、查询保留、重复 ACK 与 FIFO、停止未确认后 ACK/淘汰仍占用、内部停机及独立程序的配置启动/RPC/SIGTERM 收尾。测试替身只证明这些编排与协议行为，不证明真实隔离。

在可绑定回环端口的宿主环境执行 RPC 测试，通过。另执行 `ACORN_ISOLATE_BOX_ID=202 cargo test -p acorn-worker --locked --test rpc -- --ignored --nocapture`，一个真实 gRPC 验收测试通过，实际完成 C/Python 各两个用例的 AC、轮询与 ACK。P03 的配额和宿主隔离验收未在本阶段重新运行。

Worker 独立交付点已完成；下一步 P06 实现后端 TOML、日志、SQLx 数据库访问与测试用户预置。

### 7.7 P06：已完成

已提供 [Backend 配置示例](../config/backend.example.toml)、TOML 校验和英文 stderr 日志，分别约束 HTTP 请求体、原始任务、gRPC 消息及结果大小。入口共用 Tokio runtime 打开已有 SQLite 文件，固定 SQLx 连接池上限为 1，启用外键和有限 busy timeout，检查结构版本 1 与配置用户；不自动创建数据库、建表、迁移或装入样例。

已实现[数据库模块](../crates/backend/src/database/mod.rs)：题目当前／指定版本、语言限额、提交详情与筛选列表、幂等键查询及启动排队读取。创建使用 `BEGIN IMMEDIATE`，在事务内再次查重、固定最新版本并装配完整任务；同键同内容保留原 ID 和版本，内容冲突明确报错。源码和用例按 BLOB 绑定；返回前提交事务、释放连接，不跨越 Scheduler 交接。条件状态更新和终态保存防止迟到写入覆盖终态，并提供旧在途记录的 `SchedulerInterrupted` 写入方法。启动编排仍在 P09 接入。

[终态类型](../crates/backend/src/submissions.rs)覆盖 AC、CE、首个失败用例及执行失败，独立保存 verdict，字节字段使用 Base64，未知统计保持缺失、真实零值保留。开发用 [seed-user.sql](../examples/seed-user.sql) 提供用户 1 和合法 Argon2id 编码哈希；生成时已实际验证哈希，随机原密码未保留，不构成登录凭据或鉴权实现。示例题继续使用原 `sql/002-example-problem.sql`，初始结构与题目脚本均未修改。

已执行 `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 和 `cargo test -p acorn-backend --locked`，均通过。九个后端测试使用真实临时 SQLite 数据库和现有 SQL 脚本，覆盖查询与发布后版本固定、八个同键并发请求只插入一条记录、内容冲突、主要终态和二进制数据往返、可选统计、条件写入与终态不可覆盖、启动排队顺序和容量、连续用例校验、外键、缺失文件／用户、错误结构版本及有限锁等待。独立程序测试确认配置与数据库检查、英文日志及明确的未实现服务退出。首次检查发现测试中的多余 `Vec`，改为数组后 Clippy 通过。

本阶段未启动 HTTP 或 Scheduler，未运行新的 Worker／Isolate 验收。当前后端入口完成数据库检查并关闭连接后，以状态码 1 报告服务尚未实现；数据库模块可由测试及后续组件调用。下一步 P07 实现内存队列、Worker 客户端与调度闭环。

### 7.8 Isolate 限额单位：已修复

定位到 `--cg-mem` 与 `--fsize` 参数构造均将字节预算直接除以 1024。原有阶段校验会在初始化前拒绝换算后的 0，因此不能据此断言实际以无限额运行；但 RPC 和启动配置仍接受字节单位，存在不足 1 KiB 的预算延后失败及非整 KiB 预算向下取整的问题。

已将内存限额从题库、后端配置、RPC、Worker 配置与语言计划统一为 `memory_kib`／`max_run_memory_kib`，单文件限额改为 `file_size_max_kib`，原有磁盘空间配额仍为 KiB。Isolate 参数直接使用已校验的 KiB 正整数，不再除以 1024；启动校验同时限制支持的 KiB 整数范围。产物导出、输出保留及实际资源统计仍按字节计量，产物预算与文件／磁盘预算比较时使用 KiB 乘以 1024。

已同步设计、Proto、SQL、配置示例和调用方，通过原构建流程重新生成协议代码。按用户确认的开发期约定，直接将 `RunLimits` 字段号 3 改为 `memory_kib`，删除旧字段保留声明和兼容测试，不增加兼容分支。SQLite 结构版本升为 2，内存列改为 `memory_kib`，示例题仍为 256 MiB、保存 262144 KiB。旧数据库明确拒绝，旧配置也不兼容；部署端需同步更新服务，并离线转换旧库或初始化新库，不自动迁移或修改现存数据库。

已执行 `cargo fmt --all -- --check`、`cargo check --workspace --locked` 和 `cargo clippy --workspace --all-targets --locked -- -D warnings`，均通过。协议简化前，宿主环境执行 `cargo test --workspace --locked -- --test-threads=1`，三十六个常规测试通过，三个真实验收默认忽略；其中三个新增测试覆盖 1 KiB 配置与产物字节边界、1／1023 KiB 参数原值传递、零值和超范围在初始化前拒绝。既有数据库查询测试补充从题库到 RPC 的 KiB 数值保持、1 KiB 新版本任务及旧结构拒绝检查。旧字段兼容测试在本次确认后删除，其此前通过记录不代表当前继续提供兼容能力。

协议直接修改后已再次运行上述格式、workspace 编译与 Clippy 命令，均通过；重新生成协议并在宿主环境执行同一串行 workspace 测试命令，三十五个常规测试通过，三个真实验收默认忽略。设计中的 Proto 代码块与协议源文件一致，文档本地链接检查通过。

首次默认并发执行 `cargo test --workspace --locked` 时，原有 `deadline_stops_execution_and_reports_failure` 的 300 ms 期限测试收到 `StopUnconfirmed`，与预期的 `JobDeadlineExceeded` 不同，命令失败；随后串行执行时该测试与其余常规测试通过。该期限测试及运行停止策略未修改，默认并发运行的时序敏感性仍需单独定位，不能将串行结果记为默认并发命令通过。

在宿主环境用专用 box 203 执行 `ACORN_ISOLATE_BOX_ID=203 ACORN_ISOLATION_SENTINEL=host-only-value cargo test -p acorn-worker --locked --test isolate_real -- --ignored --nocapture`，一个真实 Isolate 阶段验收通过，包含新增的 1 KiB 单文件上限检查：写入 1024 字节成功，再写入 1 字节得到 `EFBIG`，文件长度保持 1024。该次验收也实际覆盖原有 C/Python 阶段、输出停止、共享空间／inode 配额及宿主隔离检查。真实 Executor 和 RPC 专项验收本次未重新运行。

### 7.9 P07：已完成

已实现 [`Scheduler`](../crates/backend/src/scheduler/mod.rs)：短同步锁保护 FIFO 和按 Submission ID 去重的本地记录，信号量作为待接纳名额的唯一计数。许可随创建预留、排队和等待接纳的任务转移，明确接纳或本地结束后释放，忙时退回队首继续持有。节点缓存保存当前 Job、配置地址和再次尝试时刻，按轮转选择；各节点的 [`tracking`](../crates/backend/src/scheduler/tracking.rs) 独立运行并回送完成事件，不因一个 Worker 的 RPC 等待阻塞其他节点。

[`worker_client`](../crates/backend/src/scheduler/worker_client.rs) 复用地址对应的 Tonic channel，三个调用分别设置服务端可见 deadline 和本地 timeout，应用独立发送／接收消息上限。分派先条件提交 `queued → dispatching`；明确接纳后写 `running` 并归还待接纳名额，固定地址和 ID 查询。只对明确忙、结果区满或停机拒绝退回队首并立即选择其他候选节点；提交响应不明只查询一次，查不到或无法确认时保存失败，正常查询失败也不重交旧 Job。

响应在 RPC 边界验证身份、状态、阶段、进度和终态对应关系，再复用 `TerminalOutcome` 验证原始字节和结果形状。终态提交成功后才执行一次 ACK；ACK 失败仅记警告，迟到结果不覆盖终态。已确认的后端失败代码 `WorkerUnavailable`、`WorkerJobNotFound`、`InvalidWorkerResponse` 写入后端设计；无效响应保存失败且不 ACK。关键写入失败使调度任务返回错误，未提交结果不 ACK。

新增的受控 gRPC 测试使用真实回环监听和 SQLite，覆盖忙时立即改派、两个节点同时跟踪不同提交、等待接纳占用名额、重复与过期投递、一次查询确认、失联不重交、非法响应、提交先于 ACK 及 ACK 失败。另通过关闭共享连接池验证结果写入失败时调度退出且没有 ACK；该测试不代表真实磁盘故障验收。真实 Worker 验收和本提交检查记录如下。

为拆分独立提交，P07 的测试直接预留名额、创建数据库记录并交接任务，不依赖 HTTP 或创建服务。此提交状态实际执行：

- `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- 在宿主执行 `cargo test --workspace --locked`：四十二个常规测试通过（后端十六个、Worker 二十四个、协议两个），四个真实验收默认忽略。采用默认并发方式；第 7.8 节的历史期限测试时序问题仍未单独定位。
- 在宿主使用专用 box 204 执行 `ACORN_ISOLATE_BOX_ID=204 cargo test -p acorn-backend --locked --test workflow real_scheduler_acceptance -- --ignored --nocapture`：一个真实验收通过。直接交接已入库任务，经 Scheduler、gRPC、Isolate 和 SQLite 完成 C/Python 各四用例 AC、C CE 和 Python 首例 WA；Worker 未确认区及 ACK FIFO 均设为 1，验证保存后 ACK、连续接纳及最早结果淘汰。此项不覆盖 HTTP，也未重跑 P03 全部隔离／配额或 Worker 专项验收。

当前程序入口保留 P06 行为，HTTP 与入口组装在 P08 接入。

### 7.10 P08：已完成

已实现 [`http`](../crates/backend/src/http/mod.rs) 的六个 JSON API 操作、独立 DTO、分页筛选、统一英文错误响应、`DefaultBodyLimit` 和由 Tower `ServiceBuilder` 组合的 `TraceLayer`。框架 JSON／路径／查询解析错误及路由、方法错误均转换为设计中的错误码；严格拒绝未允许的创建字段和列表参数，不向题目查询返回测试数据，不向历史返回源码或诊断。按用户确认，两个列表使用 `items`、`limit`、`offset` 分页对象，不额外查询总数。

[`CreationService`](../crates/backend/src/submissions/creation.rs) 先查同键内容，仅 queued 且未在内存中的记录需要重新交接；新建及重新交接先取得调度许可，在既有短事务中再次查重、固定版本并装配任务，提交后交给 Scheduler。查重竞争、回滚和重复投递释放多余许可。创建操作由独立 Tokio 任务持有，HTTP 仅等待结果；用实际 SQLite 写锁阻塞创建并中止 HTTP 等待者，验证解锁后仍完成保存和判题。八个同键并发请求只创建一个提交；题目发布后旧键和缺失本地记录的 queued 重交接均保持原版本，新键选择最新版本。队列满的新键不建记录，已知提交继续可查，queued 重交接失败带原 ID。

[后端入口](../crates/backend/src/main.rs) 已组装共享池、空内存 Scheduler 和实际 Axum HTTP 监听，监督两个后台任务；收到 Ctrl-C／SIGTERM 后停止新创建和分派，等待已开始的短创建操作、在途结果与一次 ACK。独立进程测试通过实际 HTTP 查询题目并用 SIGTERM 正常退出。入口尚不装入已有 queued 或中断旧 dispatching/running；创建关键写入等完整故障退出边界也仍属 P09，未将本阶段记作 P09 完成。

本阶段已执行：

- `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- `cargo test -p acorn-backend --locked`：阶段中的后端常规测试通过；真实验收默认忽略。
- 在宿主执行 `cargo test --workspace --locked`：最终四十九个常规测试通过（后端二十三个、Worker 二十四个、协议两个），四个真实验收默认忽略。此次采用命令的默认并发方式，原期限测试本次通过；第 7.8 节记录的并发失败及其未定位原因仍保留，未修改该测试或停止策略，不能据一次通过宣称时序敏感性已解决。
- 在宿主使用专用 box 204 执行 `ACORN_ISOLATE_BOX_ID=204 cargo test -p acorn-backend --locked --test workflow real_http_worker_acceptance -- --ignored --nocapture`：一个真实验收通过。实际通过 HTTP 监听、Scheduler、gRPC、Isolate 和 SQLite 完成 C/Python 各四用例 AC、C CE 和 Python 首例 WA，并查询持久化详情；Worker 未确认区及 ACK FIFO 均设为 1，连续任务可接纳且最早结果已按 ACK FIFO 淘汰。本次没有重跑 P03 的全部隔离／配额检查或原 Worker Executor、RPC 专项，不能扩大验收范围。

最初离线解析缺少新增的 tower-http，首次在线获取因 crates.io DNS 失败；后续获取成功并由 Cargo 更新锁文件。新增测试首次编译漏导入 SQLx `Connection` trait，补齐后编译及 Clippy 通过。未修改生成协议、数据库结构或原 Worker 实现；最初完成实现时未提交 Git，后按用户要求拆分为 P07 Scheduler、P08 HTTP 两次提交。项目说明已更新启动、HTTP 示例、真实验收命令和当前限制；下一步 P09。

拆分提交后，在最终 P08 状态再次执行格式检查、workspace 编译、Clippy、默认并发 workspace 测试和上述真实 HTTP 验收，均通过：四十九个常规测试通过，四个真实验收默认忽略，另单独执行的真实 HTTP 验收一个通过。文档本地链接与锚点检查及 `git diff --check` 通过。

### 7.11 P09：已完成

[后端入口](../crates/backend/src/main.rs) 在绑定 HTTP 和启动分派前调用旧在途中断与排队读取：旧 `dispatching/running` 保存为 `failed / SchedulerInterrupted`，已有终态保持不变；`queued` 按 `created_at_ms, id` 装配固定版本任务，超出容量明确退出并给出配置修改建议。[Scheduler](../crates/backend/src/scheduler/mod.rs) 在运行调度循环前装入全部任务及其信号量许可，不向旧任务查询、重交或补发 ACK。

[数据库模块](../crates/backend/src/database/mod.rs) 在共同操作边界保留 SQLx 原始错误，区分普通只读失败与致命存储失败。题目、历史、结果及创建前幂等查询的普通失败记录英文日志，HTTP 返回 `500 InternalError`，后端继续运行；创建写事务及业务状态、终态和启动中断写入的数据库错误，以及明确的 I/O、磁盘满、损坏和无效数据库文件错误，通知入口故障退出。共享通知保留首个致命错误，创建事务的报告不依赖 HTTP 等待者是否仍在连接。

正常停机停止新创建与新分派，等待短创建、在途跟踪、保存与一次 ACK，期间查询继续可用，新创建返回 `503 BackendStopping`，未分派记录保留 `queued`。修正了已开始创建仍在幂等查询时遇到停机、随后无法预留名额的问题：创建服务登记短操作后可继续尝试取得名额并完成交接，容量限制仍生效。致命故障及正常停机期间新出现的致命故障，均在收尾短创建操作后结束其余本地跟踪并尽快退出，不等待其他 Worker，未保存结果不 ACK。Worker 沿用已有内部停止和共同收尾实现。

新增八个常规测试，其中六个通过实际后端子进程、HTTP 端口、临时 SQLite 和受控 gRPC 验证启动固定版本与 FIFO、启动名额与超限、旧在途中断且终态不变、普通查询与创建前幂等查询的锁等待失败及解锁恢复、创建写入失败时不等待其他 Worker、正常停机后排队重启，以及停机等待期间终态写入失败优先退出且不 ACK。另一个创建生命周期测试使用受控首次轮询确认已开始但尚未执行幂等查询的操作仍可在停止后完成交接；严重读取测试通过实际临时文件库触发 `SQLITE_NOTADB`，验证致命分类和通知。关键写入失败分别由真实外部写锁及临时触发器拒写产生；这些结果不构成真实磁盘满或硬件 I/O 故障验收。

已执行：

- `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- 在宿主权限环境执行 `cargo test -p acorn-backend --locked`：三十一个常规测试通过，一个真实 Isolate 验收默认忽略。最终代码按默认并发方式运行通过。
- 在宿主权限环境执行 `cargo test -p acorn-worker --locked --test rpc`：六个常规测试通过，一个真实 RPC 验收默认忽略，覆盖现有 SIGTERM 收尾及停止未确认时保留执行占用。

新增测试首次编译存在创建 future 的借用生命周期错误，改用独立创建服务句柄后通过；临时配置最初使用值解析接口导致完整 TOML 文档解析失败，改用 `toml::from_str`。严重读取测试最初的附加库连接及内存库夹具未触发预期文件错误，改为单连接的实际临时文件库后通过。工具沙箱中的后端测试先因 HTTP 回环监听 `Operation not permitted` 失败，随后在宿主权限环境通过，未将环境限制记作后端功能故障。

已同步项目说明和相关设计的当前进度。此阶段没有修改 Worker 实现、Proto、数据库结构或依赖，没有重跑真实 Isolate／HTTP 判题验收和完整 workspace 测试；第 7.8 节的历史期限测试时序问题仍保留，不能由本阶段的定向测试宣称解决。下一步 P10 完成整体联调、源码样例与操作说明。

### 7.12 P10：已完成

新增 `examples/submissions/` 中的五份源码：C/Python 两数求和 AC、明确的 C 编译错误、Python 首例 WA 和持续计算的 C TLE。扩展[真实 HTTP 验收](../crates/backend/tests/workflow.rs)，直接读取这些文件；共同的 HTTP、Scheduler、真实 gRPC Worker 和 Isolate 组装提取到[真实验收夹具](../crates/backend/tests/support/real.rs)，复用正式 SQL 和已有数据库夹具。正常判题结论均保存为 `finished`，并核对 HTTP 与数据库终态详情一致。

单 Worker 真实验收通过 C/Python 各四用例 AC、C CE 零运行用例、Python WA 和 C TLE 各首例短路；核对 CE 诊断、WA 输出及 TLE 失败详情。未确认区和 ACK FIFO 均设为 1，后续样例仍可接纳，最早记录已按 ACK FIFO 淘汰。双 Worker 真实验收使用两个独立地址和 box，观察两个 HTTP 提交同时处于首例执行阶段，各自保存 TLE 并完成跟踪。随后直接用 gRPC 占用一个节点，实际确认 `WorkerBusy`，HTTP 提交在该占用仍运行时经另一节点完成 AC；测试重试间隔为 60 秒，整个等待短于该间隔，不能以定时重试代替立即转交。

[项目说明](../README.md)补齐实际二进制构建、复制配置与 box ID 选择、新库 SQL、从源码文件生成 JSON、新键与同键重传、按 `Location` 有界轮询、历史查询、双 Worker 配置、日志关联、停止顺序及重启查询。说明仍要求部署端预先准备 Linux、Isolate、控制组和用户配额，不安装或更改系统配置。

本次宿主环境：Linux `6.18.40.1-microsoft-standard-WSL2`，Rust/Cargo `1.101.0-nightly`，protoc `3.21.12`，Isolate `2.6`，GCC `15.2.0`，Python `3.14.4`，SQLite CLI `3.46.1`。Isolate 可执行文件由 root 所有并启用 setuid；沙箱根目录位于启用 `quota,usrquota` 的 ext4 文件系统。最初环境没有 `sqlite3` 命令，用户安装后按既定 SQLite CLI 流程完成初始化，未为此增加应用依赖或运行期回退路径。

已执行：

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --locked`：通过。
- 在宿主权限环境执行 `cargo test -p acorn-backend --locked`：三十一个常规测试通过，两个真实验收默认忽略。
- 在宿主使用专用 box 204 执行 `ACORN_ISOLATE_BOX_ID=204 cargo test -p acorn-backend --locked --test workflow real_http_worker_acceptance -- --ignored --nocapture`：一个真实验收通过，覆盖上述五份样例及 ACK 淘汰。
- 在宿主使用专用 box 205、206 执行 `ACORN_ISOLATE_BOX_ID=205 ACORN_ISOLATE_BOX_ID_2=206 cargo test -p acorn-backend --locked --test workflow real_two_workers_acceptance -- --ignored --nocapture`：一个真实验收通过，覆盖不同节点同时执行和忙拒绝后的立即转交。
- 启动两个独立 Worker 及一个后端程序。数据库由 `sqlite3 -bail` 依次执行三个正式／开发预置 SQL；请求使用 `curl --data-binary`，按 `Location` 查询。五份样例全部得到预期终态，同键重传返回原 ID，历史查询包含全部五条记录。正常停止后端、使用原库重启，再查询五条详情，结果与重启前完全相同；两次后端退出及两个 Worker 退出均为 0。

日志核对确认终态保存先于 ACK；C 各编译一次，Python 不编译，AC 用例按 1–4 顺序执行，CE 未运行用例，WA/TLE 只运行首例。README 提供可重复的操作命令。

本阶段没有修改业务实现、Proto、数据库结构、配置门限或 Cargo 依赖，也没有重跑完整 workspace 测试、P03 的全部隔离／配额矩阵或 Worker 专项测试。第 7.8 节的历史期限测试并发敏感性继续保留，未由本次验收宣称解决。P10 的约定交付项均已实际验证，P01–P10 首版开发计划完成。文档格式、稳定编号、七份文档的九十一处本地链接／锚点、README 十四个 shell 块和两个内嵌 Python 块的语法，以及 `git diff --check` 均通过。

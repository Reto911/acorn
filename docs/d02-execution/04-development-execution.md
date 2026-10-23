# Worker 与后端实施记录

对应设计：[Worker 与后端开发计划](../04-development-plan.md)。

当前状态：P01–P10 已完成，首版整体联调和操作验收已通过。后续健康观测见[前端实施记录](06-frontend-development-execution.md#58-f08h01worker-状态-rpc)，LLM 增量见[审查实施记录](08-llm-code-review-execution.md#12-实施记录)，MySQL 迁移及正式部署反馈见[迁移实施记录](09-mysql-migration-execution.md)。

以下保留原开发计划第 7 节的章节编号、阶段产物、实际检查、失败及未执行范围。SQLite 和早期用例数量均按当时的检查理解。

## 7. 实施记录

### 7.1 P01：已完成

已建立包含 `acorn-protocol`、`acorn-worker`、`acorn-backend` 的 Cargo workspace，采用 Rust 2024 edition。依赖由 `Cargo.lock` 锁定，本次使用 Tonic / tonic-prost / tonic-prost-build 0.14.6、Prost 0.14.4、Clap 4.6.7。

[worker.proto](../../proto/acorn/worker/v1/worker.proto) 与 Worker 设计第 3.2 节的 Proto 一致，包含三个 unary RPC、终态和退出信息的 `oneof`、可选失败用例及统计字段。[构建脚本](../../crates/protocol/build.rs) 将消息和客户端、服务端生成到 Cargo 的 `OUT_DIR`，通过 `acorn_protocol::worker::v1` 导出；Worker 的 `rpc` 模块引用服务端接口，后端的 `scheduler::worker_client` 引用客户端。

两个程序入口均支持必填的 `--config <PATH>`、`--help` 和 `--version`。当前传入路径只完成参数解析，随后明确提示服务尚未实现并以状态码 1 退出；不读取 TOML 或启动服务。配置解析分别在 P02、P06 补齐。构建与入口说明见[项目说明](../../README_zh-CN.md)。

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

已提供 Worker TOML 配置类型与 [worker.example.toml](../../config/worker.example.toml)，校验门限和预算关系，并以启动用户检查 Isolate、GCC、Python 的 `--version`。日志使用 tracing 单行英文文本、时间戳和配置级别，输出到 stderr，不继承环境日志级别。C/Python 适配器只生成命令数组、有限环境、编译预算和文件规则；C 编译一次得到固定 `program`，Python 无预编译，使用 `-I -B -S main.py`。

已运行 `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test -p acorn-worker --locked`。三个 Worker 测试通过，覆盖示例语言计划、代表性的无效预算和缺失工具。有效配置的入口检查确认英文日志写入 stderr；入口随后明确提示 RPC 尚未实现并返回 1，未宣称服务启动。下一步 P03 验证真实沙箱接口。

### 7.4 P03：已完成

已实现 [Isolate 阶段接口](../../crates/worker/src/sandbox/mod.rs)：初始化时设置空间及 inode 配额，复制阶段文件，以默认隔离规则、有限环境和显式资源预算执行；`/dev/shm` 映射为配额文件系统上的私有临时目录。异步持续收集 stdout/stderr，编译诊断共用有界保留预算，运行输出超限触发 SIGTERM。管理器退出码 0/1 确认停止后才解析元数据、独立导出固定常规产物并调用清理。清理失败记 warn；停止未确认返回独立错误并保留资源和占用。Job 期限和内部停止请求均通过同一停止路径处理。

已执行 `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 和 `cargo test -p acorn-worker --locked`。九个常规 Worker 测试通过，覆盖原 P02 三项，以及元数据缺失与 SIGKILL 不推断 OOM、固定产物导出边界、有界编译诊断与清理警告、配额初始化失败不执行、异常管理器保留占用和内部停机停止。默认命令跳过需要宿主权限的真实测试。

真实验收使用 Isolate 2.6、普通用户、专用 box、cgroup v2，以及启用用户配额的 ext4 沙箱文件系统。管理员允许普通用户初始化后，实际完成带配额初始化和清理。真实验收在宿主执行环境运行。

通过 [Worker 说明中的命令](../../crates/worker/README.md#isolate-阶段检查)单独执行一个真实验收测试，结果通过，实际覆盖：

- C 编译并导出程序，两次使用独立程序副本计算得到 `42`；Python 命令正常执行。取得实际 CPU/墙钟统计，阶段结束后调用清理。
- Python 持续输出触发 OLE，保留 4096 字节 stdout 和截断标记，原始停止原因保持为输出超限；确认停止后清理。
- 分别使用 512 KiB 空间配额和 12 inode 配额，在 `/box`、`/tmp`、`/dev/shm` 累计写入，三处均可写且累计达到同一配额时得到 `EDQUOT`。
- 沙箱不能访问实际存在的宿主私有文件，不继承宿主测试环境变量和额外可继承 FD，也不能连接同时存在的宿主 loopback 监听端口。

管理器异常退出和配额设置失败由测试替身触发，只验证对应故障处理。尚未实现 Job 判题结论、失败短路或对外 RPC；下一步 P04 使用此接口实现串行判题执行器。

### 7.5 P04：已完成

已实现 [`Executor`](../../crates/worker/src/executor.rs)：共用语言计划与完整阶段接口，C 只编译一次、Python 直接运行，各用例复制原始程序文件并串行执行；按固定尾部六种 ASCII 空白规则比较，首个失败立即短路。返回 AC、CE 或唯一首个失败用例，保留真实可选统计；已确认超限按 MLE、OLE、TLE 选择。阶段与完成数通过同步回调提供给后续 RPC 层。

Job 总期限和内部停止沿用 P03 的停止通道。普通结束在返回终态前尝试删除私有文件并释放请求和产物；停止未确认保留 Job 上下文和执行占用。失败诊断限制为 512 个 UTF-8 字节，代码与职责见 Worker 设计第 5.3 节。

已执行 `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 和 `cargo test -p acorn-worker --locked`。十五个常规 Worker 测试通过，其中新增六个执行器测试覆盖 C AC/CE/WA、编译诊断截断、Python 独立源码副本与语法错误 RE、首个失败短路、精确比较、总期限、超限优先级及 OLE 有界结果。测试管理器调用真实 GCC/Python，但不提供隔离，这些结果只证明执行编排。

使用专用 box 201 在宿主执行环境单独运行 `ACORN_ISOLATE_BOX_ID=201 cargo test -p acorn-worker --locked --test executor -- --ignored --nocapture`，一个真实验收测试通过，覆盖 C AC/CE/WA、Python AC/RE 及完成用例数。工具沙箱内同一命令先因 Isolate 初始化返回 `Must be started as root` 失败，随后在宿主权限环境通过；未把工具沙箱失败记作宿主部署故障，也未重新宣称 P03 全部隔离项目再次验收。RPC 尚未实现，下一步 P05。

### 7.6 P05：已完成

已实现 [`WorkerService`](../../crates/worker/src/rpc.rs)、[内存记录与 ACK 队列](../../crates/worker/src/jobs.rs)，并将 [Worker 入口](../../crates/worker/src/main.rs) 接入实际 Tonic 监听。三个 unary RPC 使用既定状态与原因；配置传输上限，校验新请求的语言、用例、字节数及限额。ID 查重先于内容校验，登记前在同一接纳临界区复查；接纳即返回，后台执行不绑定提交连接。运行进度保存在内存，终态在收尾后发布，记录仅含有界查询结果。

查询不改变结果保留；未确认区满时拒绝新任务，ACK 后按定长 FIFO 淘汰，重复 ACK 不调整顺序。执行占用与结果记录分开，停止未确认时失败可查，ACK 或记录淘汰均不释放位置。Ctrl-C/SIGTERM 停止新接纳并沿 P03/P04 通道停止当前执行、等待收尾；超时或仍未确认停止返回错误，不 abort 执行 future 或宣称成功。后端入口仍为骨架。

已执行 `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`，均通过。在宿主环境执行 `cargo test -p acorn-worker --locked --test rpc`，新增六个常规测试通过；随后 `cargo test --workspace --locked` 的二十三个常规测试通过（Worker 二十一个、协议两个），三个真实验收测试默认忽略。覆盖八个并发同 ID 请求只执行一次、无效内容的重复请求、忙时不登记、连接更换后继续执行、结果区满、查询保留、重复 ACK 与 FIFO、停止未确认后 ACK/淘汰仍占用、内部停机及独立程序的配置启动/RPC/SIGTERM 收尾。测试替身只证明这些编排与协议行为，不证明真实隔离。

在可绑定回环端口的宿主环境执行 RPC 测试，通过。另执行 `ACORN_ISOLATE_BOX_ID=202 cargo test -p acorn-worker --locked --test rpc -- --ignored --nocapture`，一个真实 gRPC 验收测试通过，实际完成 C/Python 各两个用例的 AC、轮询与 ACK。P03 的配额和宿主隔离验收未在本阶段重新运行。

Worker 独立交付点已完成；下一步 P06 实现后端 TOML、日志、SQLx 数据库访问与测试用户预置。

### 7.7 P06：已完成

已提供 [Backend 配置示例](../../config/backend.example.toml)、TOML 校验和英文 stderr 日志，分别约束 HTTP 请求体、原始任务、gRPC 消息及结果大小。入口共用 Tokio runtime 打开已有 SQLite 文件，固定 SQLx 连接池上限为 1，启用外键和有限 busy timeout，检查结构版本 1 与配置用户；不自动创建数据库、建表、迁移或装入样例。

已实现[数据库模块](../../crates/backend/src/database/mod.rs)：题目当前／指定版本、语言限额、提交详情与筛选列表、幂等键查询及启动排队读取。创建使用 `BEGIN IMMEDIATE`，在事务内再次查重、固定最新版本并装配完整任务；同键同内容保留原 ID 和版本，内容冲突明确报错。源码和用例按 BLOB 绑定；返回前提交事务、释放连接，不跨越 Scheduler 交接。条件状态更新和终态保存防止迟到写入覆盖终态，并提供旧在途记录的 `SchedulerInterrupted` 写入方法。启动编排仍在 P09 接入。

[终态类型](../../crates/backend/src/submissions.rs)覆盖 AC、CE、首个失败用例及执行失败，独立保存 verdict，字节字段使用 Base64，未知统计保持缺失、真实零值保留。开发用 [seed-user.sql](../../examples/seed-user.sql) 提供用户 1 和合法 Argon2id 编码哈希；生成时已实际验证哈希，随机原密码未保留，不构成登录凭据或鉴权实现。示例题继续使用原 `sql/002-example-problem.sql`，初始结构与题目脚本均未修改。

已执行 `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 和 `cargo test -p acorn-backend --locked`，均通过。九个后端测试使用真实临时 SQLite 数据库和现有 SQL 脚本，覆盖查询与发布后版本固定、八个同键并发请求只插入一条记录、内容冲突、主要终态和二进制数据往返、可选统计、条件写入与终态不可覆盖、启动排队顺序和容量、连续用例校验、外键、缺失文件／用户、错误结构版本及有限锁等待。独立程序测试确认配置与数据库检查、英文日志及明确的未实现服务退出。首次检查发现测试中的多余 `Vec`，改为数组后 Clippy 通过。

本阶段未启动 HTTP 或 Scheduler，未运行新的 Worker／Isolate 验收。当前后端入口完成数据库检查并关闭连接后，以状态码 1 报告服务尚未实现；数据库模块可由测试及后续组件调用。下一步 P07 实现内存队列、Worker 客户端与调度闭环。

### 7.8 Isolate 限额单位：已修复

定位到 `--cg-mem` 与 `--fsize` 参数构造均将字节预算直接除以 1024。原有阶段校验会在初始化前拒绝换算后的 0，因此不能据此断言实际以无限额运行；但 RPC 和启动配置仍接受字节单位，存在不足 1 KiB 的预算延后失败及非整 KiB 预算向下取整的问题。

已将内存限额从题库、后端配置、RPC、Worker 配置与语言计划统一为 `memory_kib`／`max_run_memory_kib`，单文件限额改为 `file_size_max_kib`，原有磁盘空间配额仍为 KiB。Isolate 参数直接使用已校验的 KiB 正整数，不再除以 1024；启动校验同时限制支持的 KiB 整数范围。产物导出、输出保留及实际资源统计仍按字节计量，产物预算与文件／磁盘预算比较时使用 KiB 乘以 1024。

已同步设计、Proto、SQL、配置示例和调用方，通过原构建流程重新生成协议代码。按用户确认的开发期约定，直接将 `RunLimits` 字段号 3 改为 `memory_kib`，删除旧字段保留声明和兼容测试，不增加兼容分支。SQLite 结构版本升为 2，内存列改为 `memory_kib`，示例题仍为 256 MiB、保存 262144 KiB。旧数据库明确拒绝，旧配置也不兼容；部署端需同步更新服务，并离线转换旧库或初始化新库，不自动迁移或修改现存数据库。

已执行 `cargo fmt --all -- --check`、`cargo check --workspace --locked` 和 `cargo clippy --workspace --all-targets --locked -- -D warnings`，均通过。协议简化前，宿主环境执行 `cargo test --workspace --locked -- --test-threads=1`，三十六个常规测试通过，三个真实验收默认忽略；其中三个新增测试覆盖 1 KiB 配置与产物字节边界、1／1023 KiB 参数原值传递、零值和超范围在初始化前拒绝。既有数据库查询测试补充从题库到 RPC 的 KiB 数值保持、1 KiB 新版本任务及旧结构拒绝检查。旧字段兼容测试在本次确认后删除，其此前通过记录不代表当前继续提供兼容能力。

协议直接修改后已再次运行上述格式、workspace 编译与 Clippy 命令，均通过；重新生成协议并在宿主环境执行同一串行 workspace 测试命令，三十五个常规测试通过，三个真实验收默认忽略。设计中的 Proto 代码块与协议源文件一致，文档本地链接检查通过。

首次默认并发执行 `cargo test --workspace --locked` 时，原有 `deadline_stops_execution_and_reports_failure` 的 300 ms 期限测试收到 `StopUnconfirmed`，与预期的 `JobDeadlineExceeded` 不同，命令失败；随后串行执行时该测试与其余常规测试通过。该并发时序问题后续已完成处理。

在宿主环境用专用 box 203 执行 `ACORN_ISOLATE_BOX_ID=203 ACORN_ISOLATION_SENTINEL=host-only-value cargo test -p acorn-worker --locked --test isolate_real -- --ignored --nocapture`，一个真实 Isolate 阶段验收通过，包含新增的 1 KiB 单文件上限检查：写入 1024 字节成功，再写入 1 字节得到 `EFBIG`，文件长度保持 1024。该次验收也实际覆盖原有 C/Python 阶段、输出停止、共享空间／inode 配额及宿主隔离检查。真实 Executor 和 RPC 专项验收本次未重新运行。

### 7.9 P07：已完成

已实现 [`Scheduler`](../../crates/backend/src/scheduler/mod.rs)：短同步锁保护 FIFO 和按 Submission ID 去重的本地记录，信号量作为待接纳名额的唯一计数。许可随创建预留、排队和等待接纳的任务转移，明确接纳或本地结束后释放，忙时退回队首继续持有。节点缓存保存当前 Job、配置地址和再次尝试时刻，按轮转选择；各节点的 原 tracking（已移除，现由[事件处理器](../../crates/backend/src/scheduler/kafka/events.rs)接替） 独立运行并回送完成事件，不因一个 Worker 的 RPC 等待阻塞其他节点。

[`worker_client`](../../crates/backend/src/scheduler/worker_client.rs) 复用地址对应的 Tonic channel，三个调用分别设置服务端可见 deadline 和本地 timeout，应用独立发送／接收消息上限。分派先条件提交 `queued → dispatching`；明确接纳后写 `running` 并归还待接纳名额，固定地址和 ID 查询。只对明确忙、结果区满或停机拒绝退回队首并立即选择其他候选节点；提交响应不明只查询一次，查不到或无法确认时保存失败，正常查询失败也不重交旧 Job。

响应在 RPC 边界验证身份、状态、阶段、进度和终态对应关系，再复用 `TerminalOutcome` 验证原始字节和结果形状。终态提交成功后才执行一次 ACK；ACK 失败仅记警告，迟到结果不覆盖终态。已确认的后端失败代码 `WorkerUnavailable`、`WorkerJobNotFound`、`InvalidWorkerResponse` 写入后端设计；无效响应保存失败且不 ACK。关键写入失败使调度任务返回错误，未提交结果不 ACK。

新增的受控 gRPC 测试使用真实回环监听和 SQLite，覆盖忙时立即改派、两个节点同时跟踪不同提交、等待接纳占用名额、重复与过期投递、一次查询确认、失联不重交、非法响应、提交先于 ACK 及 ACK 失败。另通过关闭共享连接池验证结果写入失败时调度退出且没有 ACK；该测试不代表真实磁盘故障验收。真实 Worker 验收和本提交检查记录如下。

为拆分独立提交，P07 的测试直接预留名额、创建数据库记录并交接任务，不依赖 HTTP 或创建服务。此提交状态实际执行：

- `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- 在宿主执行 `cargo test --workspace --locked`：四十二个常规测试通过（后端十六个、Worker 二十四个、协议两个），四个真实验收默认忽略。采用默认并发方式。
- 在宿主使用专用 box 204 执行 `ACORN_ISOLATE_BOX_ID=204 cargo test -p acorn-backend --locked --test workflow real_scheduler_acceptance -- --ignored --nocapture`：一个真实验收通过。直接交接已入库任务，经 Scheduler、gRPC、Isolate 和 SQLite 完成 C/Python 各四用例 AC、C CE 和 Python 首例 WA；Worker 未确认区及 ACK FIFO 均设为 1，验证保存后 ACK、连续接纳及最早结果淘汰。此项不覆盖 HTTP，也未重跑 P03 全部隔离／配额或 Worker 专项验收。

当前程序入口保留 P06 行为，HTTP 与入口组装在 P08 接入。

### 7.10 P08：已完成

已实现 [`http`](../../crates/backend/src/http/mod.rs) 的六个 JSON API 操作、独立 DTO、分页筛选、统一英文错误响应、`DefaultBodyLimit` 和由 Tower `ServiceBuilder` 组合的 `TraceLayer`。框架 JSON／路径／查询解析错误及路由、方法错误均转换为设计中的错误码；严格拒绝未允许的创建字段和列表参数，不向题目查询返回测试数据，不向历史返回源码或诊断。按用户确认，两个列表使用 `items`、`limit`、`offset` 分页对象，不额外查询总数。

[`CreationService`](../../crates/backend/src/submissions/creation.rs) 先查同键内容，仅 queued 且未在内存中的记录需要重新交接；新建及重新交接先取得调度许可，在既有短事务中再次查重、固定版本并装配任务，提交后交给 Scheduler。查重竞争、回滚和重复投递释放多余许可。创建操作由独立 Tokio 任务持有，HTTP 仅等待结果；用实际 SQLite 写锁阻塞创建并中止 HTTP 等待者，验证解锁后仍完成保存和判题。八个同键并发请求只创建一个提交；题目发布后旧键和缺失本地记录的 queued 重交接均保持原版本，新键选择最新版本。队列满的新键不建记录，已知提交继续可查，queued 重交接失败带原 ID。

[后端入口](../../crates/backend/src/main.rs) 已组装共享池、空内存 Scheduler 和实际 Axum HTTP 监听，监督两个后台任务；收到 Ctrl-C／SIGTERM 后停止新创建和分派，等待已开始的短创建操作、在途结果与一次 ACK。独立进程测试通过实际 HTTP 查询题目并用 SIGTERM 正常退出。入口尚不装入已有 queued 或中断旧 dispatching/running；创建关键写入等完整故障退出边界也仍属 P09，未将本阶段记作 P09 完成。

本阶段已执行：

- `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- `cargo test -p acorn-backend --locked`：阶段中的后端常规测试通过；真实验收默认忽略。
- 在宿主执行 `cargo test --workspace --locked`：最终四十九个常规测试通过（后端二十三个、Worker 二十四个、协议两个），四个真实验收默认忽略。此次采用命令的默认并发方式，原期限测试本次通过。
- 在宿主使用专用 box 204 执行 `ACORN_ISOLATE_BOX_ID=204 cargo test -p acorn-backend --locked --test workflow real_http_worker_acceptance -- --ignored --nocapture`：一个真实验收通过。实际通过 HTTP 监听、Scheduler、gRPC、Isolate 和 SQLite 完成 C/Python 各四用例 AC、C CE 和 Python 首例 WA，并查询持久化详情；Worker 未确认区及 ACK FIFO 均设为 1，连续任务可接纳且最早结果已按 ACK FIFO 淘汰。本次没有重跑 P03 的全部隔离／配额检查或原 Worker Executor、RPC 专项，不能扩大验收范围。

最初离线解析缺少新增的 tower-http，首次在线获取因 crates.io DNS 失败；后续获取成功并由 Cargo 更新锁文件。新增测试首次编译漏导入 SQLx `Connection` trait，补齐后编译及 Clippy 通过。未修改生成协议、数据库结构或原 Worker 实现；最初完成实现时未提交 Git，后按用户要求拆分为 P07 Scheduler、P08 HTTP 两次提交。项目说明已更新启动、HTTP 示例、真实验收命令和当前限制；下一步 P09。

拆分提交后，在最终 P08 状态再次执行格式检查、workspace 编译、Clippy、默认并发 workspace 测试和上述真实 HTTP 验收，均通过：四十九个常规测试通过，四个真实验收默认忽略，另单独执行的真实 HTTP 验收一个通过。文档本地链接与锚点检查及 `git diff --check` 通过。

### 7.11 P09：已完成

[后端入口](../../crates/backend/src/main.rs) 在绑定 HTTP 和启动分派前调用旧在途中断与排队读取：旧 `dispatching/running` 保存为 `failed / SchedulerInterrupted`，已有终态保持不变；`queued` 按 `created_at_ms, id` 装配固定版本任务，超出容量明确退出并给出配置修改建议。[Scheduler](../../crates/backend/src/scheduler/mod.rs) 在运行调度循环前装入全部任务及其信号量许可，不向旧任务查询、重交或补发 ACK。

[数据库模块](../../crates/backend/src/database/mod.rs) 在共同操作边界保留 SQLx 原始错误，区分普通只读失败与致命存储失败。题目、历史、结果及创建前幂等查询的普通失败记录英文日志，HTTP 返回 `500 InternalError`，后端继续运行；创建写事务及业务状态、终态和启动中断写入的数据库错误，以及明确的 I/O、磁盘满、损坏和无效数据库文件错误，通知入口故障退出。共享通知保留首个致命错误，创建事务的报告不依赖 HTTP 等待者是否仍在连接。

正常停机停止新创建与新分派，等待短创建、在途跟踪、保存与一次 ACK，期间查询继续可用，新创建返回 `503 BackendStopping`，未分派记录保留 `queued`。修正了已开始创建仍在幂等查询时遇到停机、随后无法预留名额的问题：创建服务登记短操作后可继续尝试取得名额并完成交接，容量限制仍生效。致命故障及正常停机期间新出现的致命故障，均在收尾短创建操作后结束其余本地跟踪并尽快退出，不等待其他 Worker，未保存结果不 ACK。Worker 沿用已有内部停止和共同收尾实现。

新增八个常规测试，其中六个通过实际后端子进程、HTTP 端口、临时 SQLite 和受控 gRPC 验证启动固定版本与 FIFO、启动名额与超限、旧在途中断且终态不变、普通查询与创建前幂等查询的锁等待失败及解锁恢复、创建写入失败时不等待其他 Worker、正常停机后排队重启，以及停机等待期间终态写入失败优先退出且不 ACK。另一个创建生命周期测试使用受控首次轮询确认已开始但尚未执行幂等查询的操作仍可在停止后完成交接；严重读取测试通过实际临时文件库触发 `SQLITE_NOTADB`，验证致命分类和通知。关键写入失败分别由真实外部写锁及临时触发器拒写产生；这些结果不构成真实磁盘满或硬件 I/O 故障验收。

已执行：

- `cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- 在宿主权限环境执行 `cargo test -p acorn-backend --locked`：三十一个常规测试通过，一个真实 Isolate 验收默认忽略。最终代码按默认并发方式运行通过。
- 在宿主权限环境执行 `cargo test -p acorn-worker --locked --test rpc`：六个常规测试通过，一个真实 RPC 验收默认忽略，覆盖现有 SIGTERM 收尾及停止未确认时保留执行占用。

新增测试首次编译存在创建 future 的借用生命周期错误，改用独立创建服务句柄后通过；临时配置最初使用值解析接口导致完整 TOML 文档解析失败，改用 `toml::from_str`。严重读取测试最初的附加库连接及内存库夹具未触发预期文件错误，改为单连接的实际临时文件库后通过。工具沙箱中的后端测试先因 HTTP 回环监听 `Operation not permitted` 失败，随后在宿主权限环境通过，未将环境限制记作后端功能故障。

已同步项目说明和相关设计的当前进度。此阶段没有修改 Worker 实现、Proto、数据库结构或依赖，没有重跑真实 Isolate／HTTP 判题验收和完整 workspace 测试。下一步 P10 完成整体联调、源码样例与操作说明。

### 7.12 P10：已完成

新增 `examples/submissions/` 中的五份源码：C/Python 两数求和 AC、明确的 C 编译错误、Python 首例 WA 和持续计算的 C TLE。扩展[真实 HTTP 验收](../../crates/backend/tests/workflow.rs)，直接读取这些文件；共同的 HTTP、Scheduler、真实 gRPC Worker 和 Isolate 组装提取到[真实验收夹具](../../crates/backend/tests/support/real.rs)，复用正式 SQL 和已有数据库夹具。正常判题结论均保存为 `finished`，并核对 HTTP 与数据库终态详情一致。

单 Worker 真实验收通过 C/Python 各四用例 AC、C CE 零运行用例、Python WA 和 C TLE 各首例短路；核对 CE 诊断、WA 输出及 TLE 失败详情。未确认区和 ACK FIFO 均设为 1，后续样例仍可接纳，最早记录已按 ACK FIFO 淘汰。双 Worker 真实验收使用两个独立地址和 box，观察两个 HTTP 提交同时处于首例执行阶段，各自保存 TLE 并完成跟踪。随后直接用 gRPC 占用一个节点，实际确认 `WorkerBusy`，HTTP 提交在该占用仍运行时经另一节点完成 AC；测试重试间隔为 60 秒，整个等待短于该间隔，不能以定时重试代替立即转交。

[项目说明](../../README_zh-CN.md)补齐实际二进制构建、复制配置与 box ID 选择、新库 SQL、从源码文件生成 JSON、新键与同键重传、按 `Location` 有界轮询、历史查询、双 Worker 配置、日志关联、停止顺序及重启查询。说明仍要求部署端预先准备 Linux、Isolate、控制组和用户配额，不安装或更改系统配置。

本次宿主环境：Linux `6.18.40.1-microsoft-standard-WSL2`，Rust/Cargo `1.101.0-nightly`，protoc `3.21.12`，Isolate `2.6`，GCC `15.2.0`，Python `3.14.4`，SQLite CLI `3.46.1`。Isolate 可执行文件由 root 所有并启用 setuid；沙箱根目录位于启用 `quota,usrquota` 的 ext4 文件系统。最初环境没有 `sqlite3` 命令，用户安装后按既定 SQLite CLI 流程完成初始化，未为此增加应用依赖或运行期回退路径。

已执行：

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --locked`：通过。
- 在宿主权限环境执行 `cargo test -p acorn-backend --locked`：三十一个常规测试通过，两个真实验收默认忽略。
- 在宿主使用专用 box 204 执行 `ACORN_ISOLATE_BOX_ID=204 cargo test -p acorn-backend --locked --test workflow real_http_worker_acceptance -- --ignored --nocapture`：一个真实验收通过，覆盖上述五份样例及 ACK 淘汰。
- 在宿主使用专用 box 205、206 执行 `ACORN_ISOLATE_BOX_ID=205 ACORN_ISOLATE_BOX_ID_2=206 cargo test -p acorn-backend --locked --test workflow real_two_workers_acceptance -- --ignored --nocapture`：一个真实验收通过，覆盖不同节点同时执行和忙拒绝后的立即转交。
- 启动两个独立 Worker 及一个后端程序。数据库由 `sqlite3 -bail` 依次执行三个正式／开发预置 SQL；请求使用 `curl --data-binary`，按 `Location` 查询。五份样例全部得到预期终态，同键重传返回原 ID，历史查询包含全部五条记录。正常停止后端、使用原库重启，再查询五条详情，结果与重启前完全相同；两次后端退出及两个 Worker 退出均为 0。

日志核对确认终态保存先于 ACK；C 各编译一次，Python 不编译，AC 用例按 1–4 顺序执行，CE 未运行用例，WA/TLE 只运行首例。README 提供可重复的操作命令。

本阶段没有修改业务实现、Proto、数据库结构、配置门限或 Cargo 依赖，也没有重跑完整 workspace 测试、P03 的全部隔离／配额矩阵或 Worker 专项测试。P10 的约定交付项均已实际验证，P01–P10 首版开发计划完成。文档格式、稳定编号、七份文档的九十一处本地链接／锚点、README 十四个 shell 块和两个内嵌 Python 块的语法，以及 `git diff --check` 均通过。

## 8. 文档职责整理

按用户要求，将基础判题、前端与健康观测、LLM 审查的阶段状态、实际过程和检查结果从设计与计划迁入独立实施记录；桌面范围确认、健康观测确认及旧 SQLite 数据库交付约定分别归入审核与方案记录。原任务标识、设计章节编号和历史实施小节编号保留，原失败及未执行范围保留。

设计与计划保留功能合同、模块职责、任务划分、执行步骤和验收要求，并按现行 MySQL 合同修正旧 SQLite 连接、结构版本及用例数量表述。Worker 的旧健康页面待接入状态已清除；正式部署完成且测试未发现问题的用户反馈见[迁移实施记录](09-mysql-migration-execution.md#12-正式部署反馈)。文档索引增加审核与实施记录导航，相关相对链接和章节引用同步调整。

本轮仅修改 Markdown。文档格式、表格、代码块、章节及任务编号、本地链接与锚点、shell／TOML 示例语法、三份历史实施正文的完整性检查及 `git diff --check` 通过；没有重新执行 Rust、Isolate、前端或真实模型验收。

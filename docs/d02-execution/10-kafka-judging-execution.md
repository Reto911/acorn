# Kafka 判题通信实施记录

对应设计：[10 Kafka 判题通信设计与实施计划](../10-kafka-judging-design.md)。

## 1. 当前状态

当前设计为 v20；K01–K04 的 13 项任务按 v18 完成，其余 9 项尚未实施。后端已提供任务发布、事件消费、异步位点提交及自动审查交接入口；Worker 已提供 Kafka 单槽消费、结果 FIFO、原分区归属及有限消息收尾服务，完成受控管理器和一条真实 Isolate 的 Kafka 检查，实际范围见[第 9 节](#9-k04-worker-消费与结果队列)。当前正式服务入口仍通过 gRPC 判题；下一步 K05 统一接入正式生命周期并移除旧业务 RPC，随后按 K06–K07 推进观测和整体交付。进度见[任务清单](#4-任务清单)，审核决定见[审核记录](../d01-review/10-kafka-judging-review.md#2-已确认决定与草案修正)。

Safe 接口修复的 S01–S06 已完成，两端均异步提交位点；Worker 保留 StreamConsumer，通过 SDK RAII 释放消费者。共享原生确认模块及 commit_timeout_ms 已移除，S06 已补齐动态／静态构建和静态 Worker 定向验证，safe 修复全部收口。按[修复步骤](../10-kafka-judging-design.md#12-safe-接口修复计划)在[第 10 节](#10-safe-接口修复跟踪)单独跟踪，不改变原 K01–K07 任务编号或历史完成记录。

## 2. 文档工作

- 明确队列项的交接阶段：终态确认后只重试任务位点提交，失去原分区归属时按发布确认状态释放；同步 K04.2、K04.3 和 KV04。
- 固化 BLOCKED 后关闭任务消费者并退出消费组、保留健康 RPC 与结果生产者的合同，同步 K04.4 和 KV04。
- 补充单 broker 内部位点 topic 配置及两端客户端禁用自动建 topic 的要求，同步配置任务、部署步骤和验收条件。
- 合并交接窗口、成员观测及链接方式的重复说明，统一运行和终态术语；压缩逐版本修订与检查流水，保留当前状态及检查结果。

## 3. 设计审核阶段检查结果

- `web/node_modules/.bin/prettier --check docs/10-kafka-judging-design.md docs/d01-review/10-kafka-judging-review.md docs/d02-execution/10-kafka-judging-execution.md`：通过。
- `python3` 文档检查：33 个本地链接／锚点及 2 个入链有效；标题编号连续，22 项任务与原编号一致且全部未勾选，KV01–KV06、R01–R37 连续唯一，审核表格行连续；7 个工作包的依赖存在且无循环，代码块及空白检查通过。
- `git diff --check`：通过。

上述记录仅覆盖设计审核阶段的文档检查；K01 的程序检查及结果见第 5 节。

## 4. 任务清单

任务定义、依赖及完成条件见[设计第 11 节](../10-kafka-judging-design.md#11-实施任务与验收)。完成改动及相应检查后勾选，并记录实际结果及遗留问题。

### 4.1 K01 依赖、配置与消息接入

任务定义见[设计 K01](../10-kafka-judging-design.md#k01-依赖配置与消息接入)。

- [x] K01.1 锁定 Kafka 依赖，接入并验证默认动态及显式静态构建。
- [x] K01.2 实现两端 Kafka 配置及禁用自动建 topic 的客户端选项，配置独立成员采集参数。
- [x] K01.3 接入既有 Protobuf 消息编解码、job_id 派生 key 及消息大小边界。

### 4.2 K02 数据库与本地接纳

任务定义见[设计 K02](../10-kafka-judging-design.md#k02-数据库与本地接纳)。

- [x] K02.1 调整运行及终态条件更新，保留首个有效终态且不修改表结构。
- [x] K02.2 对接创建事务后的本地交接，保留幂等重传和固定版本装配。
- [x] K02.3 实现接纳许可、ID 去重及启动装入状态的恢复和一次性收尾。

### 4.3 K03 后端发布及事件消费

任务定义见[设计 K03](../10-kafka-judging-design.md#k03-后端发布及事件消费)。

- [x] K03.1 实现任务发布、临时失败重试及投递确认后的条件更新。
- [x] K03.2 实现事件校验、状态推进、终态幂等及手动位点提交。
- [x] K03.3 迁移自动审查交接，重复终态不重复交接，位点提交失败仍完成本地交接。

### 4.4 K04 Worker 消费与结果队列

任务定义见[设计 K04](../10-kafka-judging-design.md#k04-worker-消费与结果队列)。

- [x] K04.1 复用接纳及执行入口，接入单槽消费、有界预取和消费组驱动。
- [x] K04.2 实现进度快照合并、结果容量预留和 FIFO 条目的交接阶段，终态确认后只重试位点提交。
- [x] K04.3 处理分区撤销、重获归属及条目按发布确认状态释放，禁止旧执行提交新归属位点。
- [x] K04.4 实现 BLOCKED 退出任务组、保留 RPC 和结果生产者，以及正常停机的有限总消息收尾。

### 4.5 K05 生命周期与旧流程移除

任务定义见[设计 K05](../10-kafka-judging-design.md#k05-生命周期与旧流程移除)。

- [ ] K05.1 接入后端启动恢复、后台监督和有限停机。
- [ ] K05.2 移除旧业务 RPC、路由轮询和结果 ACK 缓存，重新生成协议并迁移调用方。
- [ ] K05.3 同步 Worker、数据库、后端及自动审查设计的受影响合同。

### 4.6 K06 在线观测与健康页面

任务定义见[设计 K06](../10-kafka-judging-design.md#k06-在线观测与健康页面)。

- [ ] K06.1 实现独立成员观测及缓存，接入新鲜零成员／unknown 暂拒新建规则。
- [ ] K06.2 实现 Kafka metadata 健康采集、快照及只读 HTTP 表达。
- [ ] K06.3 更新 MQ 交接数值语义及健康页面，同步健康和前端设计。

### 4.7 K07 集成验收与部署交付

任务定义见[设计 K07](../10-kafka-judging-design.md#k07-集成验收与部署交付)。

- [ ] K07.1 固定目标 Kafka 版本，验证内部位点 topic 配置、业务 topic 创建及首次组位点提交。
- [ ] K07.2 执行必需静态检查、相关测试及 KV01–KV06，记录实际结果与限制。
- [ ] K07.3 更新中英文及模块使用说明，交付构建、配置与整体部署文档。

## 5. K01：依赖、配置与消息接入

### 5.1 已实现内容

- K01.1：锁定 rdkafka 0.39.0、rdkafka-sys 4.10.0+2.12.1 及传递依赖；backend／worker 默认启用 kafka-dynamic，关闭默认特性后可选择 kafka-static。backend 对 worker 的开发依赖关闭默认特性，避免静态测试合并出动态链接。
- K01.2：在两端配置加载器和示例中加入必填 Kafka 端点、topic 和各自组名，固定四个调用／收尾预算及消息上限默认值；后端独立配置 Worker 任务组、成员采集间隔、查询预算和派生新鲜期限。两端生产者／消费者的选项均禁止自动建 topic，并接入幂等生产、手动位点、classic 组协议及有界 SDK 缓冲。配置入口校验必填字段、约定范围和既有载荷预算的关系。
- K01.3：在 acorn-protocol 增加共享 records 模块，直接编解码 SubmitJobRequest 和 GetJobReply，从消息 job_id 派生 UTF-8 key；编码前检查 encoded_len，解码前检查输入字节数。超限及解码错误具有稳定错误名称，语义校验继续由两端业务处理负责。Proto、生成流程和数据库结构均沿用现有合同。
- 同步第 10 篇的字段、预算、客户端缓冲及模块分工，更新两端配置、模块使用说明和中英文构建说明。

### 5.2 实际检查与修正

初轮编译／检查发现测试夹具为 TestCase 添加了不存在的序号字段、两处多余的结构更新，以及新增配置节影响既有 LLM 默认值测试的截断。均已修正，以下为修正后的检查范围及结果。

| 检查                                                                                                                                                   | 实际结果                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------- |
| `cargo fmt --all -- --check`                                                                                                                           | 通过                                                                                                                    |
| `cargo check --workspace --all-targets --locked`                                                                                                       | 通过                                                                                                                    |
| `cargo clippy --workspace --all-targets --locked -- -D warnings`                                                                                       | 通过                                                                                                                    |
| `cargo build --workspace --locked`                                                                                                                     | backend／worker 默认动态构建通过                                                                                        |
| `cargo build --workspace --locked --no-default-features --features acorn-backend/kafka-static,acorn-worker/kafka-static`                               | backend／worker 随附 librdkafka 静态构建通过                                                                            |
| `cargo test --workspace --locked --no-default-features --features acorn-backend/kafka-static,acorn-worker/kafka-static --no-run --message-format=json` | 静态模式全部测试目标完成编译和链接，未执行完整 workspace 测试                                                           |
| `cargo test -p acorn-protocol --locked`                                                                                                                | 5 项通过，含 3 项 Kafka 消息边界／字节保留检查及 2 项既有可选字段／终态检查                                             |
| `cargo test -p acorn-backend --locked --lib --test config_results`                                                                                     | 10 项库测试、9 项配置及结果测试通过，覆盖既有运行阶段及终态校验、新配置和成员采集默认值及范围                           |
| `cargo test -p acorn-worker --locked --test config_language`                                                                                           | 5 项通过，含 Kafka 配置范围与客户端原生选项检查                                                                         |
| `cargo test -p acorn-backend --locked --no-default-features --features kafka-static --lib --test config_results --message-format=json`                 | 静态模式上述 19 项通过                                                                                                  |
| `cargo test -p acorn-worker --locked --no-default-features --features kafka-static --test config_language --message-format=json`                       | 静态模式上述 5 项通过                                                                                                   |
| `cargo tree --workspace --locked --no-default-features --features acorn-backend/kafka-static,acorn-worker/kafka-static -e features -i rdkafka-sys`     | 仅合并 cmake-build 与 Tokio，未启用 dynamic-linking                                                                     |
| `readelf -d`／`readelf -Ws`                                                                                                                            | 两端调用原生配置构造的测试产物在动态模式依赖 librdkafka.so；静态模式不依赖该共享库且包含已定义的 rd_kafka_conf_new 符号 |

链接核对通过 Cargo 的 JSON 产物信息选择实际测试可执行文件；动态模式对应测试亦使用了 `--message-format=json`。动态构建使用系统 librdkafka 2.13.0，静态构建使用随附 2.12.1 源码。用户补齐 CMake 后，构建所需依赖已满足。

真实消息检查使用既有 Compose 的宿主 Kafka 入口 localhost:9092；该 Compose 指定 apache/kafka:4.3.1、PLAINTEXT、单 broker 和 offsets.topic.replication.factor=1。首次在工具网络沙箱内运行时 Admin 调用超时，随后独立 TCP 检查确认网络访问被禁止；使用获准的宿主网络执行以下命令后通过：

```sh
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-backend --locked --test kafka -- --ignored --nocapture
```

检查创建独立 UUID 命名的 8 分区任务 topic 和 1 分区事件 topic，副本数为 1，设置对应记录上限、1 天保留期和 min.insync.replicas=1。使用两端正式客户端配置及共享编解码传递一份含 64 KiB 源码、127 个有序用例、合计 8 MiB 原始载荷的任务，以及 PREPARING 和失败终态事件；原始字节、顺序、状态及 job_id 派生 key 均保持一致。检查通过一项，结束后收到两个测试 topic 的删除确认；使用显式测试起始位点，未使用业务组或重置业务位点。

变更 Markdown 的 Prettier 格式、本地链接／锚点、标题编号、代码块及 TOML 示例检查，以及 `git diff --check` 通过。

### 5.3 完成状态与后续边界

K01.1–K01.3 完成。KV01 的编解码、消息大小、运行及终态形状复用检查和真实 Kafka 消息往返已覆盖；固定版本任务装配和事件消费中的业务关联随 K02–K04 验收。真实消息检查未执行源码，也未使用 MySQL 或 Isolate，不能作为端到端判题验收。

当前正式判题流程仍使用 gRPC。后续按现有依赖推进 K02 数据库与本地接纳、K03 后端发布及事件消费、K04 Worker 消费与结果队列；成员实际采集属于 K06。本轮没有更改用户 Compose、业务 topic、消费组位点或正式数据库，没有进行 K07 的整体部署及 KV02–KV06 验收。

### 5.4 共享 Kafka 策略提取

根据用户对两端重复代码的反馈，将数值默认值、端点／topic／消费组校验、调用预算及消息范围、基础连接参数、生产／消费确认策略和 SDK 缓冲配置统一到新 workspace 库 acorn-kafka。新 crate 继承 workspace lint，复用已有依赖。

两端 KafkaConfig 保留各自 TOML 字段、events_group_id／tasks_group_id 和任务／事件方向，通过借用已有字段的 ClientSettings 调用共享策略；两个模块均缩减为 72 行。后端独立成员观测配置也复用共享组名校验。工作区依赖关闭共享库默认特性，两端显式转发动态／静态特性。共享库不建立连接，客户端及消费生命周期仍由两端负责，消息编解码继续由 acorn-protocol 提供。

初次编译时发现从拥有字段改为借用字段后有三处多余引用，已修正。重构后实际检查：

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- `cargo test -p acorn-protocol --locked`：5 项通过；`cargo test -p acorn-backend --locked --lib --test config_results`：19 项通过；`cargo test -p acorn-worker --locked --test config_language`：5 项通过。复用现有测试检查共享策略及角色映射，本次提取未增加测试用例。
- `cargo build --workspace --locked` 和第 5.2 节的 workspace 静态构建命令：通过，覆盖新共享库及两个服务。两端分别执行第 5.2 节静态配置测试命令，19 项和 5 项通过；静态 Cargo 特性树包含共享库的 kafka-static／cmake-build，没有 dynamic-linking。
- 第 5.2 节真实 Kafka 往返命令：一项通过；使用重构后的两个客户端入口传递大任务和运行／终态事件，并完成独立测试 topic 删除。
- 变更及新增 Markdown 的 Prettier、本地链接／锚点、标题编号、TOML／shell 示例和中英文命令同步检查，以及 `git diff --check`：通过。

配置格式、默认值和消息合同沿用 K01；实际部署已有配置在使用新版可执行文件启动前需合并必填的 `[kafka]`，仓库示例已经提供该节，数据库、审查及沙箱配置保留部署值。本轮未修改实际部署配置、Compose、业务 topic 或消费组位点，也未扩大到 MySQL／Isolate 判题验收。

### 5.5 配置声明与组装统一

进一步集中两端仍重复的 TOML 字段、Serde 默认值标注和配置组装。acorn-kafka 使用一个仅服务现有两种角色的私有宏，从同一份声明生成 BackendConfig 与 WorkerConfig；差别为消费组字段及收发方向。共享库因此直接使用既有 serde 依赖。两端原有模块只保留对应类型的重导出，均为 4 行。

TOML 字段、默认值、未知字段拒绝规则、原有 Rust 导出路径和客户端策略沿用原合同。两端现有配置测试增加了消费组字段不得互换、消费者 group.id 必须匹配本端配置的断言，没有新增测试函数。

实际检查：

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- 两端第 5.2 节的默认配置／库测试命令：后端 19 项、Worker 5 项通过；对应静态模式测试亦为 19 项、5 项通过。
- `cargo build --workspace --locked` 及第 5.2 节的 workspace 静态构建命令：通过。
- 第 5.2 节真实 Kafka 往返命令：一项通过，确认共享类型经原两端入口完成大任务及事件往返，测试 topic 已按既有流程删除。
- 变更 Markdown 的格式、链接／锚点、标题编号、TOML／shell 示例及中英文命令同步，以及 `git diff --check`：通过。

本次只调整共享配置的源码组织，未执行 MySQL／Isolate 或完整 workspace 测试。

### 5.6 普通共享配置与本端收发方向

按用户确认，去掉配置生成宏、两个角色配置类型及 ClientSettings 借用视图，直接使用一个普通 acorn_kafka::KafkaConfig，统一声明字段、Serde 默认值、校验及通用客户端策略。`[kafka]` 消费组字段统一为 group_id：后端值仍为事件组，Worker 值仍为任务组。独立 `[worker_membership].tasks_group_id` 用于指定被观测的 Worker 组。

后端 scheduler::kafka 和 Worker kafka 保留实际职责，分别提供 producer_config／consumer_config 函数，选择各自任务／事件的收发消息上限，再调用共享策略。配置加载器、客户端调用方、两端配置及中英文和模块说明同步使用新合同。旧 Kafka 组字段按未知字段拒绝，使用现有配置时改名并保留组名值即可。

复用原有配置及真实消息测试，增加收发方向对应消息上限、group_id 必填、旧字段拒绝和消费者组名的定向断言，没有新增测试函数。检查范围及结果如下：

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- 第 5.2 节两端默认及静态模式配置／库测试：后端各 19 项、Worker 各 5 项通过。
- `cargo build --workspace --locked` 和第 5.2 节 workspace 静态构建命令：通过。
- 第 5.2 节真实 Kafka 往返命令：一项通过；通过两端方向选择函数创建四种客户端，完成大任务及事件往返，并按既有流程删除独立测试 topic。
- 变更 Markdown 的格式、链接／锚点、标题编号、TOML／shell 示例和中英文命令同步，以及 `git diff --check`：通过。

本轮未修改实际部署配置或 Compose，未执行 MySQL／Isolate 或完整 workspace 测试。

## 6. K02 数据库与本地接纳

### 6.1 实现内容

K02.1：首次 running 允许从 queued/dispatching 推进；finished/failed 允许从任一非终态保存。迟到的投递确认仍仅更新 queued，已有运行或终态不会回退。机器终态和自动审查初态保持单条条件 UPDATE 原子保存，重复终态不能替换已有结果或审查。MySQL 表结构仍为版本 4。

K02.2：复用现有 CreationService 的独立短操作、事务前许可预留、事务内再次查重与固定版本装配、提交归还连接后本地交接。完整 JudgingTask 与许可交给 Scheduler 后返回；同键沿用原 ID、版本及原始源码，内存缺失的 queued 可重新交接。回滚和重复合并仍通过许可所有权释放多余预留，创建流程没有新增网络等待或依赖。

K02.3：原本地 ID 去重记录改为同时持有可选许可，排队 payload 不再持有许可。Scheduler 的 mark_running/save_terminal 先执行数据库条件写入，成功后统一取走许可；running 和终态传输收尾期间保留不占许可的 ID，complete 在跟踪结束时清除 ID。保留的 gRPC 路径使用同一入口，ACK 和自动审查交接顺序沿用原合同。

Database::judging_recovery 和 Scheduler::restore 提供后台处理前的恢复入口：queued 按创建顺序装配完整固定版本任务并恢复许可；dispatching 只恢复 ID 和许可，不装配任务；running 只恢复 ID，不占许可。queued 与 dispatching 合计超限明确报错。旧 queued_tasks 与新恢复入口共用排队任务读取及装配流程。

同步第 02、03、08 篇中受影响的状态条件、许可归属和恢复接口说明，保留现有 gRPC 启动与 ACK 流程的描述。

### 6.2 实际检查

- `cargo fmt --all -- --check`、`cargo check -p acorn-backend --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- `cargo test -p acorn-backend --locked --test database -- --test-threads=1`：13 项通过。新增 judging_recovery_and_once_only_permit_release 一项，检查合计容量、固定任务恢复、dispatching/running 的许可区别、重复运行和终态、重复交接及关键写入失败不释放许可；现有终态和审查持久化用例调整为覆盖 queued 直接推进、首个终态不可覆盖及重复审查不替换。
- 对 workflow 选择与本次改动直接相关的 19 项现有测试，覆盖拒绝接纳与不明提交、容量、幂等与固定版本重交接、并发回滚、断开请求、终态 ACK、自动审查交接，以及旧启动／停机兼容：最终 19 项通过。初次检查有两项失败，原因是提前移除 ID 改变了旧 ACK 收尾的观察边界；已改为落库后释放许可、跟踪结束后清除 ID，再次运行同组测试通过，新增恢复用例也重新通过。
- 初次 Clippy 报告测试中两处不必要的克隆，改为借用切片后重新检查通过。

实施记录的 Prettier 格式检查通过；4 份变更 Markdown 的 90 个本地链接／锚点有效，既有标题编号、代码块和 22 项任务编号检查通过，其中 6 项已完成。`git diff --check` 通过。第 02、03、08 篇的全文 Prettier 检查仍报告格式差异，对 HEAD 原文检查确认三份文档已有同类问题；本轮沿用其既有格式，仅修改相关段落。

### 6.3 后续边界

K02.1–K02.3 完成。KV02 的数据库、本地接纳和恢复接口已检查；正式进程仍使用 gRPC 启动路径，新的等待恢复入口尚未由 main 调用。K03 接入 Kafka 发布、事件校验与消费位点，以及终态审查交接；K05 切换正式启动恢复和生命周期，同时移除旧在途中断路径。

本轮未执行完整 workflow、真实 Kafka 或 Isolate 判题检查，不能据此认为 Kafka 端到端链路已经完成。部署配置和数据库结构不需要调整。

## 7. K03 后端发布及事件消费

### 7.1 实现内容

K03.1：scheduler::kafka::Publisher 从 K02 的有界队列取得完整任务，以原 Submission ID 编码并发送。发送占用由现有 ID 记录保持，数据库连接在网络等待前归还。broker 确认后仅条件更新 queued → dispatching；临时失败保留同一编码任务及许可，按既有重试间隔继续。首次合法运行或终态落库后停止后续重发，已经在途的发送正常取得一次结果；迟到确认不会回退状态。编码超限或无法继续的客户端／配置错误通过发布服务报告并保留持久状态；一次发送失败不作为任务已无法继续的证据。

K03.2：EventConsumer 串行读取事件，复用提取后的 scheduler::validation。Database::judging_context 仅读取当前状态与固定版本的语言、用例数、review_mode，不加载源码或用例内容。首次运行使用 K02 的条件写入及许可收尾；后续合法运行快照不重复写状态。可信身份下的无效事件保存 InvalidWorkerResponse；无法解码、ID 无效或无法关联提交时记录消息位置并结束消费服务，保留未提交位点。

位点通过 librdkafka 显式提交结果队列确认；原生调用集中在一个小模块，队列和事件分别使用 RAII 释放。等待放在阻塞线程并传入原生有限超时，同时受应用 commit_timeout_ms 约束；确认读取总体与分区错误，仅提交当前已处理记录的下一位点。未增加依赖，复用现有 rdkafka 导出的绑定。

K03.3：Scheduler::persist_terminal 共用 prepare_automatic、save_terminal 及重复条件更新的审查许可收尾，原 RPC 跟踪也使用该入口。机器终态及审查初态先原子提交，归还判题许可，再尝试确认事件位点。确认成功、失败或超时均继续本次审查的本地 handoff，之后清除本地 ID；确认失败仍向调用方返回错误，消费服务不跨过该记录继续处理。重复终态保留首次结果和审查 ID，不新建审查。

新增 4 个测试函数：发布重试／早到进度窗口、关键写入后才能确认事件、提交失败／超时仍交接审查、真实后端 Kafka 链路。其余校验、固定版本、许可及审查检查复用并扩展现有用例。

workflow 的 Kafka 与审查用例放在 [tests/cases](../../crates/backend/tests/cases/mod.rs)，通过普通 mod 声明加载；support 仅保留共享夹具和辅助代码。两组用例仍属于同一个 workflow 集成测试目标，保留既有模块名供测试命令筛选。

### 7.2 实际检查与修正

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- `cargo test -p acorn-backend --locked --lib`：11 项通过。最终定向复验 3 项 Scheduler 库测试，覆盖四个运行阶段、终态对应、临时失败重试和早到进度后停止重发，全部通过。
- 定向执行 4 项 database 和 12 项 workflow 测试：全部通过。覆盖固定版本事件元信息（包括不同版本的用例数和 review_mode）、状态／审查不可覆盖、首次运行与重复快照、关键 UPDATE 失败不发起确认、未知提交拒绝确认，以及已有 gRPC ACK 和自动审查交接的兼容路径。使用独立 MySQL 夹具和既有受控模型端点。
- 用例目录整理后，格式检查与 workspace 全目标 Clippy 再次通过；`cargo test -p acorn-backend --locked --test workflow -- --list` 编译通过并保留全部 43 个用例。定向复验 kafka_commit_errors_preserve_automatic_handoff 和默认动态链接的 real_kafka_backend_publication_and_events，两项通过。
- 真实后端链路命令如下，默认动态链接与静态链接各一项通过。检查完整任务原始字节、dispatching 仍持许可、首次 RUNNING 释放许可、首次 AC 不被迟到失败覆盖，以及事件消费组实际位点推进。结束后收到独立测试 topic 与事件组的删除确认，独立测试库由夹具清理。

```sh
# 同时设置既有 MySQL 测试变量 ACORN_TEST_DATABASE_CONFIG 与 MYSQL_PASSWORD。
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-backend --locked --test workflow real_kafka_backend_publication_and_events -- --ignored
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-backend --locked --no-default-features --features kafka-static --test workflow real_kafka_backend_publication_and_events -- --ignored
```

初次数据库用例因 DROP TRIGGER 不支持 prepared statement 协议失败，改用既有 raw_sql 执行后通过。初期编译中的未使用导入及 SDK 私有 API 调用也已修正。

真实 Kafka 检查最初在手动异步提交回调上反复超时；SDK 日志确认 broker 已返回提交成功，而该手动接口未向 ConsumerContext 回传本次结果。改用显式结果队列等待实际确认后通过，并移除临时日志与轮询代码。该问题通过单项真实链路定位，未把异步提交入队当成位点确认成功。

实施记录、Kafka 设计和 backend README 的整体 Prettier 检查通过；第 03、08 篇仅检查本次新增段落，均通过并沿用已有全文格式。5 份变更 Markdown 的 102 个本地链接／锚点有效，既有标题保持稳定，新增第 03 篇 7.4 节及本记录第 7 节；代码块与 22 项任务编号检查通过，其中 9 项已完成。`git diff --check` 通过。

### 7.3 完成状态与后续边界

K03.1–K03.3 完成，KV01/KV03 的后端事件校验、发送窗口及结果确认已作上述检查。Publisher 与 EventConsumer 提供独立可调用服务，调用方传入停止信号并监督错误；正式 main 尚未切换。K04 实现 Worker 的 Kafka 接纳、执行和结果交接；K05 负责两端正式生命周期、有限停机编排及旧 RPC 移除；成员观测和 Kafka 健康仍归 K06。

本轮真实消息由测试端模拟 Worker 收发，不执行源码或 Isolate，也不代表完整 Kafka 判题端到端验收。没有改动部署配置、业务 topic、业务组位点或 MySQL 表结构。

## 8. K04 开工安排

本节保留实施前的任务安排，实际实现及检查结果见第 9 节。

### 8.1 依据与交付边界

下一工作包为 K04，前置 K01 已完成；K02–K03 同时提供后续联调所需的本地接纳、任务发布及事件处理入口。K03 的真实 Kafka 检查使用模拟 Worker，尚未覆盖任务消息进入 Worker、执行源码和回传终态的路线。K04 以[设计第 6 节](../10-kafka-judging-design.md#6-worker-消费与执行)、[错误合同](../10-kafka-judging-design.md#83-错误处理)和 [KV04](../10-kafka-judging-design.md#113-验收清单)为依据，交付可独立调用、停止和监督的 Worker Kafka 服务。

当前代码的执行编排和校验集中在 [rpc.rs](../../crates/worker/src/rpc.rs)，[jobs.rs](../../crates/worker/src/jobs.rs)保存执行位置及旧查询／ACK 记录；[kafka.rs](../../crates/worker/src/kafka.rs)仅提供客户端配置。[Executor](../../crates/worker/src/executor.rs)已提供同步进度回调、绝对期限、内部停止通道及 is_available，可直接复用。

K04 提供服务内部的接纳、执行、交接和关闭能力；K05 统一挂接两端正式入口、移除旧业务 RPC／查询记录／ACK 缓存并同步相关设计。K06 接入成员观测、Kafka 健康及 MQ 结果数量的 RPC／HTTP／页面展示。K07 负责整体部署与完整主要判题路线验收。

K04 沿用 v18“取得本次提交结果”的合同，复用后端已有的有限等待确认实现，不调整确认依据。

### 8.2 实施顺序与代码入口

按 K04.1 → K04.2 → K04.3 → K04.4 实施，每步保留对应检查；分区归属元数据随首次接纳建立，撤销规则在 K04.3 补齐验收。

| 步骤  | 实施内容与入口                                                                                                                                                            | 本步可检查的结果                                                                                                                                   |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| K04.1 | 从 rpc.rs 提取必要的 Job ID／任务校验、单槽接纳和执行收尾公共入口，旧 RPC 调用同一流程；在 Worker kafka 下接入 StreamConsumer、记录解码、分区 pause/resume 及持续消费驱动 | 消费、执行与发送互相独立；忙或结果容量不足时暂停接纳，仍处理消费组事件；复用既有执行期限、停止与资源释放，不增加应用侧未执行任务队列               |
| K04.2 | 将进度回调接到同 Job 最新待发快照；接纳前预留结果容量，执行返回后将终态交给有界 FIFO；条目保存终态及原消费记录元数据，以“待发布终态／待提交任务位点”表示交接阶段          | 运行事件不阻塞执行；终态优先，取得 broker 确认后只重试位点；交接完成才释放名额，满时停接纳、释放后恢复；仅安全停止时释放执行位置及源码、用例和产物 |
| K04.3 | 消费驱动处理重平衡通知，为活动执行及结果条目保留可区分撤销前与重新分配后的本地归属；根据当前归属决定是否允许提交原记录 offset+1                                           | 撤销不等待或取消已开始的执行；旧执行不能借重新获得的同名分区提交位点；按终态发布阶段释放失去归属的条目；同分区不跨过未交接记录                     |
| K04.4 | 将执行位置进入 BLOCKED 通知消费驱动；提供消费者关闭、健康 RPC 与结果生产者分别保留的服务生命周期，以及正常停机的统一消息收尾入口                                          | BLOCKED 立即退出任务组且不等结果发送，原执行位置继续保留；正常停机先停接纳，经既有停止通道收尾执行，全部结果与位点共享一个有限消息预算             |

正式入口在 K05 切换；K04 期间通过服务接口和测试夹具启动 Kafka 路径。公共执行入口继续保证本机单槽，不另建一套沙箱执行流程。

### 8.3 实施时重点保持的合同

- 接收边界先检查编码大小并解码，再复用现有 Job ID、语言、载荷、用例及限额校验。身份明确的非法任务不执行源码，生成有界 InvalidJob／UnsupportedLanguage 终态并按正常交接顺序处理；无法解码或 Job ID 无效时记录消息位置并结束消费服务，保留未提交位点。
- 结果容量包含执行前的预留及尚未完成交接的终态条目。执行中预留不计入终态数量；消息已发布但任务位点尚未确认的条目仍占名额。进程内 Job ID 去重不能代替每条消费记录的位点及归属检查。
- 运行快照只保留同一活动 Job 的最新待发值，发送中的快照也受既有消息预算约束；终态形成后清除该 Job 尚未发送的中间快照。终态各自独立保留，不按 Job 进度合并不同任务。
- 终态已确认且保有原归属时，等待对应任务位点确认后释放条目；终态已确认但原归属已失去时，直接释放；终态未确认且原归属已失去时，继续发布，确认后释放且不提交旧任务位点。短暂断连本身不作为撤销依据。
- 位点确认复用原共享 OffsetCommit（S05 已移除）的实际结果与分区错误检查；两端共用部分只作必要提取，客户端所有权、归属和重试仍由各自服务负责。手动异步提交入队不能当作确认，提交失败不能令已确认的终态退回发布阶段。
- BLOCKED 的消费者关闭与结果发布分别推进。正常停机的 kafka.shutdown_timeout_ms 是全部条目共用的消息预算，独立于 sandbox.shutdown_timeout_ms；耗尽后报告未完成交接并保留未提交位点。核对 SDK 关闭及阻塞调用的实际收尾，不能仅凭外层 timeout 宣称消费者已经关闭；正式进程总停机编排在 K05 复验。

### 8.4 定向验收安排

优先复用 [Worker 夹具](../../crates/worker/tests/support/mod.rs)、[RPC 测试](../../crates/worker/tests/rpc.rs)、[执行器测试](../../crates/worker/tests/executor.rs)和[沙箱生命周期测试](../../crates/worker/tests/sandbox_lifecycle.rs)。Kafka 资源创建、初始位点和实际已提交位点观测沿用 [K03 真实链路测试](../../crates/backend/tests/cases/kafka_workflow.rs)的做法，仅建立本批所需夹具。

| 检查范围           | 最小检查及依据                                                                                                                                                                            |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 接纳与消费驱动     | 单槽、非法任务不执行、执行期间持续驱动消费组；受控长任务及容量不足期间不积压应用侧未执行任务。复用原期限、停止、清理警告和 BLOCKED 覆盖                                                   |
| 结果容量与交接阶段 | 容量设为 1，执行前预留，结果积压时暂停新接纳，完整交接后恢复；终态发布确认后制造位点失败，核对仅重试位点、不重新发送终态或执行源码                                                        |
| 撤销及重新分配     | 在判题期间撤销分区，核对本地执行继续、旧位点不提交；覆盖发布确认前后撤销及同一分区重新分配回来，检查条目按阶段释放、同分区位点不跨过未交接项                                              |
| BLOCKED 与正常停机 | 用已有受控管理器制造 StopUnconfirmed，观察任务消费者退出组、另一 Worker 接管未确认记录、原健康 RPC 保持 BLOCKED、生产者仍可交接终态；正常停机检查多个条目共用消息预算及预算耗尽不提前提交 |
| 真实 Kafka 与执行  | 使用独立 topic／消费组和专用 Isolate box，验证一条小任务从任务消费、真实执行到终态发布及实际任务位点确认的链路；重平衡检查使用同组两个 Worker。受控管理器检查不作为真实隔离证据           |

Rust 改动执行既有必需命令及受影响的 Worker 测试：

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p acorn-worker --locked
```

动态／静态构建复用第 5.2 节命令，定向检查新增服务在两种模式下的链接与调用。若提取共享位点确认，则复验 K03 库测试及第 7.2 节已有真实后端链路。新增 Worker Kafka 测试的实际目标和命令随实现记录，不预先记作已存在或已通过。真实检查沿用 ACORN_TEST_KAFKA_SERVERS；需要数据库联调时继续使用独立 MySQL 夹具及既有测试变量。

K04.1–K04.4 只有在对应实现及检查完成后才勾选；完整 C/Python 判题组合、双 Worker 整体联调、HTTP／GUI、成员观测与部署仍按 K05–K07 推进。本次仅完成文档梳理和开工规划，未修改 Rust、协议、SQL、配置或部署，未重新运行历史测试。

本次规划的文档检查：Prettier、27 个本地链接／锚点、既有标题保留、章节编号、代码块及 `git diff --check` 通过；22 项任务及其勾选状态保持不变，仍为 9 项完成、13 项待实施。

## 9. K04 Worker 消费与结果队列

### 9.1 实现内容

K04.1：从 rpc.rs 提取 [runtime.rs](../../crates/worker/src/runtime.rs)，共用既有输入校验、单槽接纳、绝对期限、执行与停止路径。JobRecord 使用 watch 保存最新有界快照，RPC 读取同一记录，Kafka 按变更通知处理进度及终态。新增 [KafkaWorker](../../crates/worker/src/kafka/service.rs)，调用方传入同配置的事件生产者，服务创建任务消费者；消费、执行通知、发送及关闭分别推进。忙或结果名额不足时 pause 分区，持续驱动消费组；新分配分区抢先送达的未接纳记录 seek 回原位，不增加未执行任务队列。

K04.2：以活动任务、FIFO 条目和正在交接的终态数量直接核算预留结果容量，复用 max_unacknowledged_results。进度只保留同一活动 Job 的最新待发快照，终态形成后清除尚未发送的进度。FIFO 先发布终态，确认后只重试原任务 offset+1；完整交接后才释放结果名额和对应运行记录。结果队列只保存有界终态、原消费位置和通道身份，正常安全收尾后的源码、用例及产物仍由既有执行器释放。身份明确的非法内容返回 InvalidJob／UnsupportedLanguage 终态；解码或身份错误保留消息位置并结束消费服务。

K04.3：[归属回调](../../crates/worker/src/kafka/ownership.rs)在撤销前永久失效原分区的本地标识，重新分配生成新标识。回调只更新短锁状态并通知驱动，不等待执行或网络。活动任务继续完成；已发布终态失去归属后释放，未发布终态继续发布后释放，旧执行不提交位点。提交入队与撤销使用同一短锁，实际结果等待在锁外。重新分配的记录不借用旧归属的执行结果确认，旧交接也不能删除新的同 ID 运行记录。

两端的原生位点结果队列提取到 原 acorn-kafka::commit（S05 已移除），仍以本次提交的实际结果及分区错误为依据。后端沿用一次有限原生等待；Worker 分段轮询同一个提交结果队列，观察原归属与共同消息期限，避免外层异步超时后留下长时间的原生等待。临时 Kafka 错误分类一并共用。没有新增依赖、消息封装、数据库结构或结果恢复机制。

K04.4：[消费者关闭](../../crates/worker/src/kafka/close.rs)显式发起原生异步关闭并持续驱动，观察实际 closed 后释放 SDK 句柄。BLOCKED 立即失效原归属并开始退出任务组，保留原状态 RPC 和事件生产者，服务继续等待部署端停止。正常停机先停止接纳，按既有沙箱预算处理活动执行，再为全部结果、位点和消费关闭建立一个共同消息期限；未完成时明确返回错误，不提前确认记录。正式二进制入口和旧 RPC 的统一切换仍属于 K05。

### 9.2 实际检查与修正

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- `cargo build --workspace --locked` 及第 5.2 节的 workspace 静态构建命令：通过。
- `cargo test -p acorn-worker --locked`：26 项通过，7 项按显式资源要求忽略。复用既有配置、C/Python 执行、RPC、期限、停止未确认和清理检查；新增一项库测试，注入一次位点确认失败，检查终态只发送一次、仅重试位点，以及发布后撤销不提交原记录。
- `cargo test -p acorn-backend --locked --lib`：11 项通过，覆盖既有事件／结果校验、发布窗口和审查策略。后端仅替换共享确认调用，业务持久化及审查交接顺序保持不变。
- 按第 7.2 节命令复验默认动态模式的 real_kafka_backend_publication_and_events：一项通过。测试连接信息从用户提供的 `.env` 读取，密码仅传入测试子进程；沿用既有夹具创建并清理独立数据库，核对任务发布、首次运行释放名额、终态不可覆盖及真实事件组位点推进。本次未重跑该项的静态模式。

新增四项真实 Kafka 检查，复用 Worker 的既有受控管理器及任务构造，创建独立临时 topic／消费组并初始化本组位点。以下默认动态模式四项分别通过，静态模式另执行第一项并通过：

```sh
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-worker --locked --test kafka real_kafka_worker_capacity_and_shutdown -- --ignored
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-worker --locked --test kafka real_kafka_blocked_worker_leaves_group_for_replacement -- --ignored
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-worker --locked --test kafka real_kafka_result_backpressure_and_total_shutdown_budget -- --ignored
ACORN_TEST_KAFKA_SERVERS=localhost:9092 ACORN_ISOLATE_BOX_ID=210 cargo test -p acorn-worker --locked --test kafka real_kafka_isolate_execution -- --ignored
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-worker --locked --no-default-features --features kafka-static --test kafka real_kafka_worker_capacity_and_shutdown -- --ignored
```

检查范围如下：

| 检查             | 实际覆盖                                                                                                                                                                                                              |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 容量、撤销和停机 | 容量为 1，活动任务不提前提交；判题期间引入同组被动成员撤销原分区，原执行继续并发布 AC，但组位点保持 0；分区重新分配回来后重新执行，后续任务正常推进。非法语言不启动执行器；正常停机发布 WorkerShutdown 并确认任务位点 |
| BLOCKED 交接     | 受控管理器返回停止未确认，原 RPC 方法仍报告 BLOCKED，失败终态可发布，任务位点保持 0；另一 Worker 从该位点接管并完成 AC，原服务继续运行，最终停止仍报告执行位置未安全释放                                              |
| 结果积压与总预算 | 仅为事件生产者选择不可达测试端点，任务消费者连接真实 broker；容量 2 时两项结果积压，第三任务不执行。两个结果共用 500 ms 消息预算，停机在 2 秒检查上限内明确失败，组位点仍为 0                                         |
| 真实执行         | 专用 box 210，通过真实 Kafka、Worker 和 Isolate 完成一份 Python 两数求和任务，核对 AC、DONE、完成用例数及实际任务位点 1                                                                                               |

每项真实检查均收到独立 topic 和任务组的删除确认。受控管理器的检查只证明编排合同，真实隔离路线仅限上述一份 Python 小任务；未扩展为完整 C/Python 或隔离故障矩阵。

初次普通 Worker 测试的 RPC 项因工具沙箱禁止回环监听失败；获准在宿主环境运行后通过，最终常规 Worker 测试也在同一允许监听的环境通过。早期编译中的接收器变量遮蔽、客户端类型推断、异步回调 Send 约束及 Clippy 提示均已修正。

撤销用例最初仍按未发生重投时的两次执行计数断言而失败，修正为三次后通过。总预算用例最初尝试通过单副本 topic 的 min.insync.replicas=2 制造失败，实际未阻止投递，等待结果积压超时；Kafka 对有效最小 ISR 的副本数边界见[官方说明](https://issues.apache.org/jira/browse/KAFKA-21110)。随后改为显式传入仅事件方向不可达的现有 FutureProducer，保留真实消费和关闭链路后通过，未为测试新增客户端抽象或配置项。

4 份变更文档的整体 Prettier 检查通过；第 01 篇仅检查两段变更内容，保留既有表格格式。5 份文档的 86 个本地链接／锚点、既有标题、章节编号、代码块、13 段 shell 和 3 段 TOML 示例检查通过；22 项任务保持原编号，其中 13 项完成、9 项待实施。`git diff --check` 通过。

### 9.3 完成状态与后续边界

K04.1–K04.4 完成，KV04 的单槽、原分区撤销与重获、交接阶段、结果容量、BLOCKED 接管及正常停机已作上述定向检查。正式 main 仍使用 gRPC；下一步 K05 将后端恢复、发布／事件消费和 Worker Kafka 服务纳入正式监督与停机，同时移除旧业务 RPC、查询记录和 ACK FIFO，按既有流程重新生成协议。

本批真实调用覆盖 Worker Kafka 和既有 K03 后端链路；两项检查分别验证各自边界，不能合并声称已完成 HTTP、真实 Worker 与 MySQL 的完整端到端验收。未执行完整 workflow、workspace 全套测试、HTTP／GUI 或 K07 整体验收，未修改实际部署配置、broker、业务 topic、业务组位点和 MySQL 表结构。成员观测、Kafka 健康和 MQ 结果数量的最终接线仍属于 K06。

## 10. Safe 接口修复跟踪

步骤定义、顺序及完成条件见[设计第 12 节](../10-kafka-judging-design.md#12-safe-接口修复计划)。用户已接受两端异步位点提交及 Worker 重复执行成本。S01–S06 全部完成，两端使用 safe 异步位点接口，Worker 沿用 StreamConsumer 并通过 SDK RAII 释放；共享原生确认及无用配置已删除，动态／静态构建和约定的定向验证已完成。

- [x] S01 确认 Worker 取舍并同步受影响规约。
- [x] S02 后端改为 safe 异步提交，保留持久化及审查收尾。
- [x] S03 Worker 沿用 StreamConsumer，以 SDK RAII 释放并退出消费组。
- [x] S04 Worker 改为原归属下的异步位点提交。
- [x] S05 清理原生确认实现及无用配置，更新使用说明。
- [x] S06 完成定向测试及动态／静态验证。

各步完成后勾选，并在此记录改动、实际验证命令及结果、剩余限制。

### 10.1 S01 规约同步

用户已确认两端异步位点提交、Worker 重复执行及结果名额在请求入队后释放的取舍。Kafka 设计升级为 v19，明确先完成必要业务处理、消息发布仍取得 broker 确认、位点不读回且不逐条等待回执；同步原分区归属、safe 消费关闭、发布／消息收尾预算、错误和 KV03／KV04 验收。第 01、03、08 篇已同步相应 Kafka 合同；审核记录 R38 保留决定，研究中的读回路线仍作为未采纳的比较材料。

S01 仅改文档。commit_timeout_ms 的代码字段、配置示例及 crate README 在 S05 清理；当前生产代码和部署配置未变更，没有运行 Rust、Kafka、MySQL 或沙箱测试。

本次修改的 Markdown 格式、本地链接／锚点及稳定编号检查通过，`git diff --check` 通过。Prettier 检查覆盖 Kafka 设计及配套审核、研究和实施记录全文；第 01、03、08 篇仅检查本次修改的 Kafka 段落。

### 10.2 S02 后端异步位点提交

EventConsumer 直接使用 SDK safe CommitMode::Async 入队当前记录的 offset+1，不读回、不等待 broker 回执；移除后端 OffsetCommit 调用、spawn_blocking、确认超时及不再需要的消费者 Arc。EventProcessor::process 接收同步 FnOnce 回调，校验和必要数据库写入完成后才调用；请求入队成功或失败均先交接已保存的审查，再清理终态本地 ID。入队失败保留错误并结束当前消费服务，不继续处理后续事件。

复用两项现有测试：数据库用例调整为 kafka_events_enqueue_only_persisted_changes，继续检查关键写入失败不调用提交、重复运行快照不写库、非法身份拒绝提交及许可收尾；kafka_commit_errors_preserve_automatic_handoff 改为同步入队失败，使用已有模型响应门保留 PENDING，检查审查仍完成且重放保留同一审查 ID、只发起一次模型调用。移除原确认超时分支，没有新增测试函数。

实际检查均通过：

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`。
- `cargo test -p acorn-backend --locked --lib --test config_results`：11 项库测试及 9 项配置测试通过。
- `cargo test -p acorn-backend --locked --test database kafka_events_enqueue_only_persisted_changes`：1 项通过。
- `cargo test -p acorn-backend --locked --test workflow kafka_commit_errors_preserve_automatic_handoff`：1 项通过。
- `ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-backend --locked --test workflow real_kafka_backend_publication_and_events -- --ignored`：默认动态链接下 1 项通过，覆盖任务原始字节、投递与运行许可边界、首次终态不被覆盖及实际消费组位点推进。沿用既有 MySQL 测试变量与独立夹具；本例的 topic、消费组和数据库已清理。

后端 README 已说明请求入队语义及 commit_timeout_ms 仍被共享配置接受、校验，但不再供后端使用。Worker、共享 OffsetCommit、配置字段和部署配置未修改；未运行 Worker、Isolate、静态模式或完整 workspace 测试，静态复验及驱动性能比较仍按 S06 完成。S03 的执行步骤见[消费驱动与关闭计划](../10-kafka-judging-design.md#124-s03-worker-safe-消费驱动与关闭)。

三份变更 Markdown 的 Prettier、本地链接／锚点及稳定编号检查通过，`git diff --check` 通过；原 K 任务清单保持不变，S02 已勾选，S03–S06 未完成。

### 10.3 S03 保留 StreamConsumer 与 RAII 释放

用户撤回 BaseConsumer 改型，要求保留 StreamConsumer 和原消费驱动，采用 SDK RAII 释放消费者、退出组。S03 仍需移除 K04 的原生关闭调用，已据此缩小任务并完成；Kafka 设计升级为 v20，审核决定 R39、Worker 设计、研究及使用说明同步。原计划因消费改型而安排的接收延迟／空闲 CPU 对比已撤回，静态链接复验仍在 S06。

[close.rs](../../crates/worker/src/kafka/close.rs) 移除 6 处 unsafe 及所有原生调用。服务先失效原归属，在阻塞线程等待在途提交释放 Arc，再取得独占消费者并由 SDK 析构退出组；等待最后一个引用是为避免销毁某个 Arc 后便误报释放完成。StreamConsumer、recv、pause/resume、归属回调及任务位点确认流程保持原实现。

BLOCKED 仍独立开始消费者释放，不等待终态发布，状态 RPC 和生产者继续运行。共同消息期限约束交接及等待释放；SDK 析构中的关闭错误由 SDK 日志报告。超时会明确报错，但不取消已开始的阻塞析构，不承诺进程在该期限内完全退出。

实际检查均通过：

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`。
- `cargo test -p acorn-worker --locked --lib`：3 项通过，包含既有终态只发布一次、位点失败只重试本阶段和撤销旧归属检查。
- 以下三项真实 Kafka 检查分别通过，复用既有受控管理器，没有新增测试函数。覆盖容量／撤销／正常停机、BLOCKED 后另一 Worker 接管且原 RPC／生产者可用，以及结果积压与共同消息预算；独立 topic 和消费组均已清理。

```sh
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-worker --locked --test kafka real_kafka_worker_capacity_and_shutdown -- --ignored
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-worker --locked --test kafka real_kafka_blocked_worker_leaves_group_for_replacement -- --ignored
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-worker --locked --test kafka real_kafka_result_backpressure_and_total_shutdown_budget -- --ignored
```

本轮仅验证默认动态链接及上述路径，未重跑真实 Isolate、静态模式、完整 Worker 或 workspace 测试；未验证 broker 故障下 SDK 析构的最长耗时。手写 Rust 仅共享 OffsetCommit 仍含 9 处 unsafe，任务位点异步提交由 S04 修改，模块及配置清理留到 S05。S04 的步骤见[异步任务位点计划](../10-kafka-judging-design.md#125-s04-worker-异步任务位点提交)。

五份变更使用说明与 Kafka 记录的完整 Prettier 检查通过，第 01 篇仅检查本次修改段落；全部变更文档的本地链接／锚点、稳定编号及 `git diff --check` 通过。原 K 清单和 R01–R38 决定保留，新增 R39；S03 已勾选，S04–S06 未完成。

### 10.4 S04 Worker 异步任务位点提交

S04 是删除共享确认模块的前置，本批先完成该切换，再清理 S05。[handoff::commit](../../crates/worker/src/kafka/handoff.rs) 改为同步调用 SDK safe CommitMode::Async，在同一原归属短锁内检查归属和共同消息期限，并仅入队当前原记录的 offset+1；移除单次确认预算、阻塞任务及结果等待，不读回位点。消费驱动继续使用 StreamConsumer。

终态仍须先取得发布确认，再发起位点请求；成功入队即完成当前 FIFO 交接并释放条目、结果名额及对应运行记录。同步可见的临时入队失败只重试本阶段，不重发终态；失去原归属时保留既有按阶段释放规则，不提交旧位点，也不跨过同分区未完成记录。

现有交接测试调整为 confirmed_terminal_retries_only_enqueue_and_releases_revoked_origin，注入一次同步入队错误，核对只发布一次、重试入队后完成及撤销旧归属。提交不再跨 await 持有消费者引用，RAII 释放中的 Arc::try_unwrap 等待循环一并移除；最后一个消费者仍由阻塞线程析构，共同预算和不可取消的 SDK 析构边界保持不变。

### 10.5 S05 共享确认及配置清理

删除原 acorn-kafka::commit 模块及导出，移除剩余 9 处 unsafe；共享库仅保留配置、角色无关的客户端策略、组名校验和临时错误分类。commit_timeout_ms 的字段、默认函数、范围校验和两端 TOML 示例均已移除，三个 crate README 和受影响规约同步；历史记录保留原验证结果，并注明原模块已删除，移除失效源码链接。

两端配置测试继续覆盖剩余三个时间预算的默认值和约定范围，并将旧预算校验改为明确拒绝 commit_timeout_ms 字段，没有新增测试函数。既有部署 TOML 必须删除该字段，否则按未知字段拒绝启动；本批未改动实际部署配置。

以下默认动态模式检查均通过：

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`。
- `cargo test -p acorn-worker --locked --lib --test config_language`：3 项库测试和 5 项配置／语言测试通过。
- `cargo test -p acorn-backend --locked --lib --test config_results`：11 项库测试和 9 项配置测试通过。
- 下列三项 Worker 和一项后端真实 Kafka 检查分别通过，覆盖异步入队后的容量／撤销／停机、BLOCKED 替代接管、结果积压／共同预算及后端任务发布、事件持久化和组位点推进。Worker 使用既有受控管理器，后端沿用独立 MySQL 夹具；独立 topic、消费组及本例数据库均已清理。

```sh
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-worker --locked --test kafka real_kafka_worker_capacity_and_shutdown -- --ignored
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-worker --locked --test kafka real_kafka_blocked_worker_leaves_group_for_replacement -- --ignored
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-worker --locked --test kafka real_kafka_result_backpressure_and_total_shutdown_budget -- --ignored
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-backend --locked --test workflow real_kafka_backend_publication_and_events -- --ignored
```

检索手写 Rust 已无 unsafe、原生 Kafka 绑定调用或 OffsetCommit；commit_timeout_ms 仅在两端拒绝旧字段的测试字符串中保留。未运行静态模式、真实 Isolate 或完整 workspace 测试；未验证 broker 故障下 SDK 析构的最长耗时。S06 的剩余构建、静态用例及收口检查见[收尾验证计划](../10-kafka-judging-design.md#126-s06-收尾验证)，当前正式入口仍待 K05 切换。

六份变更使用说明及 Kafka 记录的完整 Prettier 检查通过，第 03 篇仅检查本次修改段落；全部变更文档的本地链接／锚点、稳定编号、TOML 示例及 `git diff --check` 通过。原 K 任务清单保持不变，S04、S05 已勾选，S06 尚未完成。

### 10.6 S06 收尾验证

本步未修改 Rust、协议或配置，也未新增测试。复核第 10.2、10.5 节已通过的关键路径、库／配置测试及格式、workspace check、Clippy 结果，沿用这些结果；补齐最终构建、静态 Worker 用例及代码、配置和文档检查。

以下命令分别通过：

```sh
cargo build --workspace --locked
cargo build --workspace --locked --no-default-features --features acorn-backend/kafka-static,acorn-worker/kafka-static
ACORN_TEST_KAFKA_SERVERS=localhost:9092 cargo test -p acorn-worker --locked --no-default-features --features kafka-static --test kafka real_kafka_worker_capacity_and_shutdown -- --ignored
```

两种 workspace 构建均覆盖后端和 Worker。静态 Worker 的真实 Kafka 用例 1 项通过，复用既有受控管理器和独立 topic／消费组，覆盖容量、执行期间撤销原分区、重新分配后重投、非法语言不执行，以及正常停机的终态与任务位点交接；测试资源已清理。静态真实 Kafka 检查仅限该用例，其余动态路径沿用第 10.5 节结果。

链接方式通过 Cargo 特性树及实际调用 SDK 的测试产物核对。默认模式启用 dynamic-linking，后端配置测试及 Worker Kafka 测试产物依赖 librdkafka.so.1；显式静态模式启用 cmake-build，不含 dynamic-linking，静态 Worker Kafka 测试产物无该共享库依赖，并定义 rd_kafka_new。特性树使用以下命令：

```sh
cargo tree --workspace --locked -e features -i rdkafka-sys
cargo tree --workspace --locked --no-default-features --features acorn-backend/kafka-static,acorn-worker/kafka-static -e features -i rdkafka-sys
```

首次直接要求正式二进制含 librdkafka 动态依赖的断言失败：当前两端正式入口仍使用 gRPC，未使用的 Kafka 代码可被链接器裁剪，因此正式二进制的依赖表不适合作为该判据。该失败未作为链接模式结论；上述特性树及实际调用 SDK 的测试产物验证符合预期。

最终检索确认手写生产 Rust 无 unsafe、原生 Kafka 绑定调用、位点读回或同步位点提交；原共享确认模块不存在。两个 TOML 示例仅保留投递、生产者入队和总消息收尾三个时间预算；旧字段拒绝沿用第 10.5 节两端配置测试，三个 crate README 均明确要求删除 commit_timeout_ms。

S01–S06 已全部勾选，原 K01–K07 清单及历史检查记录保留。未扩大到完整 workspace 测试、真实 Isolate、静态后端 Kafka 全链路或 broker 故障下 SDK 析构最长耗时；消费驱动保持 StreamConsumer，原改型性能对比已按 R39 撤回。正式入口切换、旧业务 RPC 移除及整体端到端验收仍由 K05–K07 实施，RAII 析构不可取消的边界保持不变。

相关 10 份文档的本地链接／锚点、代码块、TOML 示例和任务编号检查通过；本次执行记录的 Prettier、既有标题与 K 清单保留检查及 `git diff --check` 通过。本步仅更新执行记录。

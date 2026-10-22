# Kafka 判题通信实施记录

对应设计：[10 Kafka 判题通信设计与实施计划](../10-kafka-judging-design.md)。

## 1. 当前状态

设计版本为 v18，K01 与 K02 的 6 项任务已完成，其余 16 项尚未开始。已接入 Kafka 依赖、配置和编解码，并完成数据库状态条件更新、本地许可收尾和等待恢复入口。当前服务入口仍通过 gRPC 判题，Kafka 发布／消费与生命周期按 K03–K06 继续实现。进度见[任务清单](#4-任务清单)，审核决定见[审核记录](../d01-review/10-kafka-judging-review.md#2-已确认决定与草案修正)。

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

- [ ] K03.1 实现任务发布、临时失败重试及投递确认后的条件更新。
- [ ] K03.2 实现事件校验、状态推进、终态幂等及手动位点提交。
- [ ] K03.3 迁移自动审查交接，重复终态不重复交接，位点提交失败仍完成本地交接。

### 4.4 K04 Worker 消费与结果队列

任务定义见[设计 K04](../10-kafka-judging-design.md#k04-worker-消费与结果队列)。

- [ ] K04.1 复用接纳及执行入口，接入单槽消费、有界预取和消费组驱动。
- [ ] K04.2 实现进度快照合并、结果容量预留和 FIFO 条目的交接阶段，终态确认后只重试位点提交。
- [ ] K04.3 处理分区撤销、重获归属及条目按发布确认状态释放，禁止旧执行提交新归属位点。
- [ ] K04.4 实现 BLOCKED 退出任务组、保留 RPC 和结果生产者，以及正常停机的有限总消息收尾。

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

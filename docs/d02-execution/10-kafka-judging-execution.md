# Kafka 判题通信实施记录

对应设计：[10 Kafka 判题通信设计与实施计划](../10-kafka-judging-design.md)。

## 1. 当前状态

设计版本为 v16，K01 的 3 项任务已完成，其余 19 项尚未开始。已接入依赖、两端配置与客户端选项、共享消息编解码，并完成两种链接构建及真实消息往返。当前服务入口仍通过 gRPC 判题，Kafka 发布／消费与生命周期按 K02–K06 继续实现。进度见[任务清单](#4-任务清单)，审核决定见[审核记录](../d01-review/10-kafka-judging-review.md#2-已确认决定与草案修正)。

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

- [ ] K02.1 调整运行及终态条件更新，保留首个有效终态且不修改表结构。
- [ ] K02.2 对接创建事务后的本地交接，保留幂等重传和固定版本装配。
- [ ] K02.3 实现接纳许可、ID 去重及启动装入状态的恢复和一次性收尾。

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

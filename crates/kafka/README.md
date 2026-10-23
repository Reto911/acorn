# acorn-kafka

acorn-kafka 为后端与 Worker 提供共享 Kafka 配置策略：预算默认值、端点／topic／消费组校验、消息大小与调用预算范围、客户端选项及临时传输错误分类。

## 接口与职责

- `KafkaConfig` 是两端共用的普通 TOML 结构体，统一声明字段、Serde 默认值和配置校验，消费组字段为 `group_id`。
- `defaults` 提供数值字段默认值。
- `KafkaConfig::producer_config(record_limit)`／`consumer_config(record_limit)` 提供与角色无关的客户端策略；调用方先完成配置校验，再传入该方向的消息上限。
- `validate_group_id` 同时供本服务消费组和后端独立成员观测配置使用，错误保留调用方字段名。
- `temporary` 统一分类可重试的 Kafka 传输及投递错误。

后端的 [scheduler::kafka](../backend/src/scheduler/kafka.rs) 选择任务生产、事件消费；[Worker kafka](../worker/src/kafka.rs) 选择事件生产、任务消费。两端模块提供 `producer_config(&KafkaConfig)`／`consumer_config(&KafkaConfig)`，根据自己的收发方向传入相应消息上限。

后端 `[kafka].group_id` 使用事件组，Worker 使用任务组。既有配置应将 `[kafka].events_group_id` 或 `[kafka].tasks_group_id` 改名为 `group_id`，保留原组名值；独立 `[worker_membership].tasks_group_id` 用于指定被观测的 Worker 任务组。旧的 `commit_timeout_ms` 字段必须删除；两端仅保留消息投递、生产者入队和总消息收尾三个时间预算。字段和默认值见[后端说明](../backend/README.md#kafka-客户端配置)及[Worker 说明](../worker/README.md#kafka-客户端配置)。

共享库构造配置时不建立连接。Kafka 客户端的创建、发布、消费、分区归属、位点重试和生命周期由两端业务模块负责。两端直接调用 SDK safe 异步提交接口，仅请求入队，不等待逐条 broker 回执；不再提供自有位点确认队列。Protobuf 消息及其编解码由 [acorn-protocol](../protocol/README.md#kafka-消息编解码)负责。

## 单 broker 部署

部署基线固定为 Apache Kafka **4.3.1**，采用单 broker，业务 topic 的副本数和 `min.insync.replicas` 均为 1。broker 需持久化数据，并配置：

```properties
auto.create.topics.enable=false
offsets.topic.replication.factor=1
message.max.bytes=16778240
replica.fetch.max.bytes=16778240
```

后端和 Worker 的 `bootstrap_servers` 须与实际部署一致，broker 广播的地址也须能被客户端访问。以下管理命令以 `localhost:9092` 为例，在可连接 broker 且 Kafka 自带 `bin` 目录已加入 `PATH` 的终端执行。

### 启动 broker

按实际部署方式启动 broker，再核对 CLI 版本和 metadata 可用性：

```sh
kafka-topics.sh --version
kafka-topics.sh --bootstrap-server localhost:9092 --list
```

版本应为 `4.3.1`，metadata 列表命令应成功。

### 首次创建 topic 和初始化位点

以下步骤只用于新部署，执行时后端和所有 Worker 尚未启动。普通重启沿用现有 topic 和消费组位点，直接启动服务。

```sh
kafka-topics.sh --bootstrap-server localhost:9092 \
  --create --topic acorn.judge.tasks.v1 --partitions 8 --replication-factor 1 \
  --config min.insync.replicas=1 --config cleanup.policy=delete \
  --config retention.ms=86400000 --config max.message.bytes=16778240
kafka-topics.sh --bootstrap-server localhost:9092 \
  --create --topic acorn.judge.events.v1 --partitions 1 --replication-factor 1 \
  --config min.insync.replicas=1 --config cleanup.policy=delete \
  --config retention.ms=86400000 --config max.message.bytes=1049600
```

任务和事件的编码上限分别为 16 MiB、1 MiB，topic 记录上限各增加 1024 字节，覆盖记录开销；broker 的 `message.max.bytes` 和 `replica.fetch.max.bytes` 也配置为 16778240。两个 topic 的保留期均为一天，采用 `delete` 策略，实际日志清理由 Kafka 按日志段执行。

确认创建成功后，首次初始化两个组的全部分区位点：

```sh
kafka-consumer-groups.sh --bootstrap-server localhost:9092 \
  --group acorn-judge-workers-v1 --topic acorn.judge.tasks.v1 \
  --reset-offsets --to-earliest --execute
kafka-consumer-groups.sh --bootstrap-server localhost:9092 \
  --group acorn-backend-judge-events-v1 --topic acorn.judge.events.v1 \
  --reset-offsets --to-earliest --execute
```

这里使用 Kafka 管理端的位点设置命令，见[官方消费组管理说明](https://kafka.apache.org/43/operations/basic-kafka-operations/)。新建空 topic 的起始位点均为 0；任务组应输出 8 个分区，事件组输出 1 个分区。初始化命令会创建内部 `__consumer_offsets`，因此单 broker 的内部位点 topic 副本数必须预先设为 1。这些命令会改变位点，仅在本节规定的新部署阶段执行。

### 核对并启动应用

```sh
kafka-topics.sh --bootstrap-server localhost:9092 \
  --describe --topic acorn.judge.tasks.v1
kafka-topics.sh --bootstrap-server localhost:9092 \
  --describe --topic acorn.judge.events.v1
kafka-topics.sh --bootstrap-server localhost:9092 \
  --describe --topic __consumer_offsets
kafka-consumer-groups.sh --bootstrap-server localhost:9092 \
  --describe --group acorn-judge-workers-v1
kafka-consumer-groups.sh --bootstrap-server localhost:9092 \
  --describe --group acorn-backend-judge-events-v1
```

核对业务 topic 分区为 8／1、副本数为 1、全部分区有 leader；`Configs` 包含上述记录上限、保留期和策略。内部位点 topic 的副本数应为 1。两个组分别有 8／1 条 `CURRENT-OFFSET=0`，不能是缺失值 `-`。此时没有活动消费者属于正常状态。任一步骤报错时先停止初始化并处理部署错误，再开始后续步骤。

后端和 Worker 都使用上述 topic：后端 `[kafka].group_id` 为 `acorn-backend-judge-events-v1`，Worker `[kafka].group_id` 为 `acorn-judge-workers-v1`，后端 `[worker_membership].tasks_group_id` 与 Worker 的组名一致。核对两份示例 TOML 后，按[快速开始](../../README_zh-CN.md#启动服务)启动 Worker、后端和前端。应用保持 `auto.offset.reset=error`，没有位点时明确报告错误。

### 普通停止与重启

先停止后端和 Worker，再按实际部署方式停止 broker。重启时先启动 broker，再启动应用；保留 broker 数据、topic 和消费组位点，不重复执行首次初始化。需要核对进度时运行只读 `--describe` 命令。

## 构建与检查

此 crate 继承 workspace 的 Rustdoc 和 Clippy 规则。默认启用 `kafka-dynamic`；workspace 依赖关闭其默认特性，由 backend／worker 显式传递相同的链接特性。两种构建方式见[根说明](../../README_zh-CN.md#构建)。

现有[后端配置测试](../backend/tests/config_results.rs)、[Worker 配置测试](../worker/tests/config_language.rs)和[真实 Kafka 往返检查](../backend/README.md#检查命令)通过两端入口覆盖共享策略和角色映射。

# acorn-kafka

acorn-kafka 为后端与 Worker 提供共享 Kafka 配置策略：预算默认值、端点／topic／消费组校验、消息大小与调用预算范围，以及生产者和消费者的客户端选项。

## 接口与职责

- `KafkaConfig` 是两端共用的普通 TOML 结构体，统一声明字段、Serde 默认值和配置校验，消费组字段为 `group_id`。
- `defaults` 提供数值字段默认值。
- `KafkaConfig::producer_config(record_limit)`／`consumer_config(record_limit)` 提供与角色无关的客户端策略；调用方先完成配置校验，再传入该方向的消息上限。
- `validate_group_id` 同时供本服务消费组和后端独立成员观测配置使用，错误保留调用方字段名。

后端的 [scheduler::kafka](../backend/src/scheduler/kafka.rs) 选择任务生产、事件消费；[Worker kafka](../worker/src/kafka.rs) 选择事件生产、任务消费。两端模块提供 `producer_config(&KafkaConfig)`／`consumer_config(&KafkaConfig)`，根据自己的收发方向传入相应消息上限。

后端 `[kafka].group_id` 使用事件组，Worker 使用任务组。既有配置应将 `[kafka].events_group_id` 或 `[kafka].tasks_group_id` 改名为 `group_id`，保留原组名值；独立 `[worker_membership].tasks_group_id` 用于指定被观测的 Worker 任务组。字段和默认值见[后端说明](../backend/README.md#kafka-客户端配置)及[Worker 说明](../worker/README.md#kafka-客户端配置)。

共享库构造配置时不建立连接。Kafka 客户端的创建、发布、消费、位点交接和生命周期由两端业务模块负责；Protobuf 消息及其编解码由 [acorn-protocol](../protocol/README.md#kafka-消息编解码)负责。

## 构建与检查

此 crate 继承 workspace 的 Rustdoc 和 Clippy 规则。默认启用 `kafka-dynamic`；workspace 依赖关闭其默认特性，由 backend／worker 显式传递相同的链接特性。两种构建方式见[根说明](../../README_zh-CN.md#构建)。

现有[后端配置测试](../backend/tests/config_results.rs)、[Worker 配置测试](../worker/tests/config_language.rs)和[真实 Kafka 往返检查](../backend/README.md#检查命令)通过两端入口覆盖共享策略和角色映射。

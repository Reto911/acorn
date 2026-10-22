# acorn-kafka

acorn-kafka 为后端与 Worker 提供共享 Kafka 配置策略：预算默认值、端点／topic／消费组校验、消息大小与调用预算范围，以及生产者和消费者的客户端选项。

## 接口与职责

- `defaults` 提供两端 TOML 数值字段的统一默认值。
- `ClientSettings` 借用两端已有配置字段，统一执行校验和构造 `rdkafka::ClientConfig`。调用方先完成配置校验，再传入对应收发方向的消息上限。
- `validate_group_id` 由两端消费组配置及后端独立成员观测配置共用，错误保留调用方字段名。

后端的 `events_group_id` 和 Worker 的 `tasks_group_id` 仍由各自 TOML 类型声明。后端发送任务、消费事件，Worker 发送事件、消费任务；两个入口只负责将配置字段及该方向的消息上限交给共享策略。字段和默认值见[后端说明](../backend/README.md#kafka-客户端配置)及[Worker 说明](../worker/README.md#kafka-客户端配置)。

共享库构造配置时不建立连接。Kafka 客户端的创建、发布、消费、位点交接和生命周期由两端业务模块负责；Protobuf 消息及其编解码由 [acorn-protocol](../protocol/README.md#kafka-消息编解码)负责。

## 构建与检查

此 crate 继承 workspace 的 Rustdoc 和 Clippy 规则。默认启用 `kafka-dynamic`；workspace 依赖关闭其默认特性，由 backend／worker 显式传递相同的链接特性。两种构建方式见[根说明](../../README_zh-CN.md#构建)。

现有[后端配置测试](../backend/tests/config_results.rs)、[Worker 配置测试](../worker/tests/config_language.rs)和[真实 Kafka 往返检查](../backend/README.md#检查命令)通过两端入口覆盖共享策略和角色映射。

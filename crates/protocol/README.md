# acorn-protocol

acorn-protocol 提供后端与 Worker 共用的 Protobuf 消息、Tonic gRPC 客户端和服务端接口。协议包名为 `acorn.worker.v1`，Rust 导出路径为 `acorn_protocol::worker::v1`。

## 接口

| RPC               | 请求与响应                                       |
| ----------------- | ------------------------------------------------ |
| `GetWorkerStatus` | 查询执行状态、停机标记及待 MQ 交接终态数量／容量 |

Kafka 任务使用 `SubmitJobRequest`，运行与终态事件使用 `GetJobReply`，数据字段及编号保持不变。源码、用例输入和期望输出使用 `bytes`。CPU 与墙钟限额使用毫秒，内存限额使用 KiB；结果中的时间统计使用微秒，内存统计使用字节。可选统计字段保留“未提供”和数值 0 的区别，终态使用 `oneof` 区分判题结果与执行失败。

字段定义见 [worker.proto](../../proto/acorn/worker/v1/worker.proto)，业务语义见 [Worker RPC 契约](../../docs/01-worker-design.md#3-rpc-契约)。

## 构建与生成

需要支持 Rust 2024 edition 的 Rust/Cargo 和支持 proto3 optional 的 `protoc`。在仓库根目录构建：

```sh
cargo build -p acorn-protocol --locked
```

[build.rs](build.rs) 使用 Tonic/Prost 将 Proto 生成为 Cargo `OUT_DIR` 中的 Rust 代码，[src/lib.rs](src/lib.rs) 通过 `tonic::include_proto!` 导出。修改 Proto 后重新构建即可生成新代码；生成结果由构建系统管理。

默认从 PATH 查找 `protoc`。自定义编译器路径时设置：

```sh
PROTOC=/path/to/protoc cargo build -p acorn-protocol --locked
```

## 使用

workspace 内的 crate 通过共享依赖引用：

```toml
[dependencies]
acorn-protocol.workspace = true
```

生成的接口可从以下路径导入：

```rust
use acorn_protocol::worker::v1::{
    SubmitJobRequest,
    judge_worker_client::JudgeWorkerClient,
    judge_worker_server::{JudgeWorker, JudgeWorkerServer},
};
```

[后端客户端](../backend/src/scheduler/worker_client.rs)负责调用 deadline、消息预算和响应转换；[Worker RPC 实现](../worker/src/rpc.rs)仅负责状态观测，Kafka 服务复用公共接纳与执行入口。协议 crate 负责共享类型和服务接口。

## Kafka 消息编解码

`acorn_protocol::records` 提供 `encode_task`／`decode_task` 和 `encode_event`／`decode_event`。任务复用 `SubmitJobRequest`，事件复用 `GetJobReply`，直接使用 Protobuf 二进制，不新增消息封装或字段。

编码入口只接收消息和字节上限，返回 `EncodedRecord`；其 `key()` 直接来自同一消息的 `job_id`，`payload()` 返回编码字节。发送时按 UTF-8 使用 key。接收端按负载身份执行原业务校验，不额外比较 Kafka key。

编码前检查 `encoded_len()`，解码前检查接收字节数，同时执行配置上限及任务 16 MiB／事件 1 MiB 的协议上限。超限返回 `KafkaRecordTooLarge`，无法解码返回 `KafkaRecordDecodeFailed`；错误不包含消息正文。身份、资源和状态形状仍由 Worker 接纳及后端事件处理校验，消费驱动负责关联 topic／partition／offset 并按设计处理错误。

## 检查命令

在仓库根目录执行：

```sh
cargo test -p acorn-protocol --locked
```

测试检查 Protobuf 编解码中的可选字段及终态分支。格式、编译和 Clippy 命令见[根说明](../../README_zh-CN.md#开发检查)。

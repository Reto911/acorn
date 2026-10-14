# acorn-protocol

acorn-protocol 提供后端与 Worker 共用的 Protobuf 消息、Tonic gRPC 客户端和服务端接口。协议包名为 `acorn.worker.v1`，Rust 导出路径为 `acorn_protocol::worker::v1`。

## 接口

| RPC                 | 请求与响应                                                    |
| ------------------- | ------------------------------------------------------------- |
| `SubmitJob`         | 接收任务 ID、语言、源码字节、有序用例和运行限额，返回接纳状态 |
| `GetJob`            | 按 ID 查询执行阶段、进度及判题结果或执行失败                  |
| `AcknowledgeResult` | 按 ID 确认结果已保存                                          |
| `GetWorkerStatus`   | 查询执行状态、停机标记及未确认结果容量                        |

源码、用例输入和期望输出使用 `bytes`。CPU 与墙钟限额使用毫秒，内存限额使用 KiB；结果中的时间统计使用微秒，内存统计使用字节。可选统计字段保留“未提供”和数值 0 的区别，终态使用 `oneof` 区分判题结果与执行失败。

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

[后端客户端](../backend/src/scheduler/worker_client.rs)负责调用 deadline、消息预算和响应转换；[Worker RPC 实现](../worker/src/rpc.rs)负责接纳与执行。协议 crate 负责共享类型和服务接口。

## 检查命令

在仓库根目录执行：

```sh
cargo test -p acorn-protocol --locked
```

测试检查 Protobuf 编解码中的可选字段及终态分支。格式、编译和 Clippy 命令见[根说明](../../README_zh-CN.md#开发检查)。

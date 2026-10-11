# Acorn

C 与 Python 3 的单文件源码判题系统。后端包含 HTTP API、Scheduler 和 SQLite 访问模块，通过 gRPC 调用独立 Worker。

当前已完成 P01 工程骨架与共享协议，以及 P02 Worker 配置、日志和语言计划。开发范围、执行契约和后续步骤见[文档索引](docs/README.md)与[开发计划](docs/04-development-plan.md)。

## 构建与检查

需要 Rust/Cargo（支持 Rust 2024 edition 及锁定依赖）、rustfmt、Clippy 和支持 proto3 optional 的 `protoc`。`protoc` 需在 PATH 中，或通过 `PROTOC` 环境变量指定路径。依赖版本由 `Cargo.lock` 锁定。

在仓库根目录执行：

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

格式检查、编译检查和 Clippy 是每次 Rust 代码变更的必备检查，相关测试按改动范围选择。Clippy 检查全部 workspace 成员及其库、程序、测试和示例目标，警告视为错误。

根 `Cargo.toml` 将 Rust 的 `missing_docs` 和 Clippy 的 `missing_docs_in_private_items` 均设为 `deny`，各 crate 通过 `[lints] workspace = true` 继承。自写模块、结构体、结构体字段、成员方法和函数必须有 Rustdoc 文档，覆盖公共与私有项；使用 `///` 或 `//!`，普通 `//` 注释不能替代。结构体成员变量指字段，函数内的局部变量不属于这两条 lint 的检查对象。新增 crate 也必须继承 workspace lint 配置。

唯一的生成代码豁免位于 `acorn_protocol::worker::v1`，只对 Tonic/Prost 输出局部允许上述两条文档 lint，原因随属性注明；该模块本身仍有文档。自写协议包装、Worker 和后端代码不使用全局豁免。lint 检查文档是否存在，内容的准确性仍需代码评审。

workspace 包含三个 crate：

- `acorn-protocol`：共享消息及生成的 gRPC 客户端、服务端。
- `acorn-worker`：Worker 程序入口与服务端接口。
- `acorn-backend`：后端程序入口与 Scheduler 的 Worker 客户端接口。

协议源文件为 [worker.proto](proto/acorn/worker/v1/worker.proto)，与 [Worker 设计第 3.2 节](docs/01-worker-design.md#32-消息)保持一致。`acorn-protocol` 的构建脚本调用 Tonic/Prost，在 Cargo 的 `OUT_DIR` 中生成代码，并通过 `acorn_protocol::worker::v1` 导出。修改 Proto 后再次构建即重新生成；生成文件不手工编辑、不提交到仓库。

## 程序入口

查看帮助：

```sh
cargo run --locked -p acorn-worker -- --help
cargo run --locked -p acorn-backend -- --help
```

两个入口均要求 `--config <PATH>`，并支持 `--version`。Worker 读取并校验 [worker.example.toml](config/worker.example.toml) 同形的 TOML 配置，初始化英文 stderr 日志，检查 Isolate/GCC/Python 的可调用性并生成语言计划：

```sh
cargo run --locked -p acorn-worker -- --config config/worker.example.toml
```

当前有效 Worker 配置会输出验证完成日志，随后提示 RPC 服务尚未实现并以状态码 1 退出；无效配置或不可调用的工具明确报错，不启动监听。后端仍只解析路径，配置解析和示例在 P06 提供。工具的 `--version` 检查不能替代真实沙箱验收。

真实判题需要 Linux、Isolate、C/Python 工具链，以及设计规定的控制组、用户配额和权限配置；这些前提在 P03 验证。

# Acorn

C 与 Python 3 的单文件源码判题系统。后端包含 HTTP API、Scheduler 和 SQLite 访问模块，通过 gRPC 调用独立 Worker。

当前已完成 P01–P05，Worker 可独立通过 gRPC 提供 C/Python 串行判题、结果查询和 ACK。后端业务从 P06 开始实现。开发范围、执行契约和后续步骤见[文档索引](docs/README.md)与[开发计划](docs/04-development-plan.md)。

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

有效 Worker 配置通过校验后启动 gRPC 监听，提供 `SubmitJob`、`GetJob` 和 `AcknowledgeResult`。提交立即返回，后台执行不随提交连接断开而取消；一个 Worker 同时只执行一个 Job，忙时不排队。终态须由调用方保存后 ACK，未确认结果区满时拒绝新任务。请求和结果消息上限从配置应用到 Tonic 服务端。

Ctrl-C 或 SIGTERM 先停止接纳，再通过共同停止路径结束当前执行，按 `sandbox.shutdown_timeout_ms` 等待收尾。正常停机返回 0；启动、监听、停机超时或停止未确认明确报错并返回 1。停止未确认时，运行中的服务仍可查询和 ACK 失败结果，但保持执行位置占用，由部署端处置。结果只存在本进程内存，重启不恢复。

无效配置或不可调用的工具明确报错，不启动监听。后端仍只解析路径，配置解析和示例在 P06 提供。工具的 `--version` 检查不能替代真实沙箱验收。

真实判题需要 Linux、Isolate、C/Python 工具链，以及设计规定的控制组、用户配额和权限配置。P03 已验证实际阶段执行与隔离；P04 已通过内部接口完成真实 C/Python 判题，P05 已通过实际 gRPC 客户端提交、轮询并 ACK。

P04 执行器的真实验收使用独立 box ID，按内部调用检查 C 的 AC/CE/WA、Python 的 AC/语法错误 RE，以及完成用例数：

```sh
ACORN_ISOLATE_BOX_ID=201 \
  cargo test -p acorn-worker --locked --test executor -- --ignored --nocapture
```

P05 的真实验收通过生成的 Tonic 客户端连接回环随机端口，在专用 box 202 完成 C/Python 两用例 AC、查询和 ACK：

```sh
ACORN_ISOLATE_BOX_ID=202 \
  cargo test -p acorn-worker --locked --test rpc -- --ignored --nocapture
```

常规 RPC 测试也会绑定回环随机端口，但使用受控管理器，不要求 Isolate 初始化权限。限制本机 socket 的工具沙箱需在宿主执行这些测试；受控管理器的 GCC/Python 执行不构成真实隔离证据。

## Isolate 阶段验收

部署时保留 Isolate 官方的 root 所有权和 setuid 安装方式，在沙箱根目录所在文件系统启用用户配额，并准备控制组。Worker 仍以启动用户运行。若采用普通用户初始化方式，由管理员在 `/etc/isolate` 设置 `restricted_init = 0`，再以该用户执行 `isolate --check-config`；配置加载成功仍不能替代初始化与实际运行检查。隔离和配额约定见 [Worker 设计第 7.2 节](docs/01-worker-design.md#72-基于-isolate-默认规则的隔离策略)。

默认测试运行无需 Isolate 初始化权限；真实验收单独执行，须选择专用 box ID，避免与其他 Worker 冲突。以下命令使用 box 200，测试按本机默认 `/var/lib/isolate` 根目录检查清理：

```sh
ACORN_ISOLATE_BOX_ID=200 ACORN_ISOLATION_SENTINEL=host-only-value \
  cargo test -p acorn-worker --locked --test isolate_real -- --ignored --nocapture
```

该测试实际编译 C、使用独立程序副本执行两次并运行 Python，检查输出超限停止、三处可写目录共享空间及 inode 配额，以及宿主私有文件、继承环境、额外 FD 和宿主监听端口的隔离。配额设置失败、管理器异常退出和清理警告另由可控测试覆盖；停止未确认时跳过依赖停止的清理并保留执行占用，需由部署端处置。

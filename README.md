# Acorn

C 与 Python 3 的单文件源码判题系统。后端包含 HTTP API、Scheduler 和 SQLite 访问模块，通过 gRPC 调用独立 Worker。

当前已完成 P01–P08：Worker 独立提供 C/Python 串行判题，后端通过 HTTP 创建提交、进程内 Scheduler 分派、gRPC 取结果、SQLite 持久化和 ACK 完成业务闭环。下一步 P09 补齐启动装入及故障边界。开发范围、执行契约和后续步骤见[文档索引](docs/README.md)与[开发计划](docs/04-development-plan.md)。

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

无效配置或不可调用的工具明确报错，不启动监听。工具的 `--version` 检查不能替代真实沙箱验收。

后端读取 [backend.example.toml](config/backend.example.toml)，初始化英文日志，打开已初始化数据库并检查结构版本及预置测试用户，然后启动 Scheduler 和 HTTP 监听。数据库路径相对于启动目录；示例使用 `acorn.db`、用户 1 和 `http://127.0.0.1:50051` Worker endpoint。先启动上文的 Worker，再在另一终端于仓库根目录准备新数据库并启动后端：

```sh
sqlite3 -bail acorn.db < sql/001-initial-schema.sql
sqlite3 -bail acorn.db < examples/seed-user.sql
sqlite3 -bail acorn.db < sql/002-example-problem.sql
cargo run --locked -p acorn-backend -- --config config/backend.example.toml
```

三个 SQL 脚本仅在新库执行一次。应用不会创建数据库、装入 SQL 或执行升级。测试用户脚本仅供开发，Argon2id 哈希来自已丢弃的随机密码；当前没有登录功能或可用登录凭据。示例题仍随正式发布保留。

后端收到 Ctrl-C 或 SIGTERM 后停止创建接纳和新分派，等待已开始的短创建操作、在途判题、结果保存及一次有 deadline 的 ACK，再关闭连接池。Scheduler 异常结束会使后端退出。当前入口从空内存队列开始，已有 `queued` 可由同键重传重新交接；启动自动装入、旧 `dispatching/running` 中断写入和创建关键写入故障的完整退出编排仍在 P09 接入。

查询题目并提交 Python 两数求和源码：

```sh
curl http://127.0.0.1:8080/api/v1/problems
curl -i http://127.0.0.1:8080/api/v1/submissions \
  -H 'Content-Type: application/json' \
  -H 'Idempotency-Key: 7b5f28d3-51a4-463c-98a4-0a11815d8d46' \
  -d '{"problem_id":1,"language_id":"python","source":"a,b=map(int,input().split());print(a+b)\n"}'
```

创建返回 `202`、`submission_id`、`state` 和 `Location`；同键同内容重传返回 `200`，新的主动提交更换 UUID 键。按返回的 `Location` 轮询详情即可取得持久化终态；提交历史为 `GET /api/v1/submissions`。两个列表都返回 `{"items":[...],"limit":20,"offset":0}`，支持 `limit`、`offset`；详情中的终态字节字段使用 Base64。六个路由、筛选和英文错误码见[HTTP 契约](docs/03-backend-design.md#5-http-接口)。

内存限额和 Isolate 单文件／磁盘空间限额统一使用 KiB（1 KiB = 1024 字节），配置字段明确以 `_kib` 结尾；实际资源统计、输出保留和产物导出大小仍按字节计量。数据库结构版本为 2，内存列为 `memory_kib`；旧配置与旧数据库需更新，后端与 Worker 按当前 KiB 协议同步构建运行。已有数据库由部署端离线转换或另建新库，详见 [SQL 使用说明](sql/README.md)。

P06–P08 的数据库、Scheduler 和 HTTP 常规测试：

```sh
cargo test -p acorn-backend --locked
```

P07/P08 的真实验收通过实际 HTTP 端口、Scheduler、gRPC、SQLite 和 Isolate 检查 C/Python AC、C CE、Python 首例 WA，以及保存与 ACK 淘汰。使用独立 box ID，在具备既定权限的宿主环境执行：

```sh
ACORN_ISOLATE_BOX_ID=204 \
  cargo test -p acorn-backend --locked --test workflow real_http_worker_acceptance -- --ignored --nocapture
```

后端使用一个共享 SQLx SQLite 连接池，固定最多一个连接；外键和有限锁等待配置用于每次连接建立。创建提交以 `BEGIN IMMEDIATE` 固定最新版本和幂等键，在返回完整内存任务前提交事务并释放连接。状态及终态使用条件更新，AC/CE/首个失败用例和执行失败按类型化 JSON 保存，字节字段使用 Base64。配置中的 HTTP JSON 上限、gRPC 二进制上限和原始任务／结果上限分别计量，示例值与 Worker 示例对齐。

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

该测试实际编译 C、使用独立程序副本执行两次并运行 Python，检查输出超限停止、1 KiB 单文件上限触发 `EFBIG`、三处可写目录共享空间及 inode 配额，以及宿主私有文件、继承环境、额外 FD 和宿主监听端口的隔离。配额设置失败、管理器异常退出和清理警告另由可控测试覆盖；停止未确认时跳过依赖停止的清理并保留执行占用，需由部署端处置。

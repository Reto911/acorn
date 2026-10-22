# acorn-worker

acorn-worker 是独立的 C/Python 判题进程，通过 gRPC 接收任务，在 Isolate 沙箱中编译和逐例执行，返回进度、判题结论及诊断。

## 执行方式

- 每个 Worker 同时执行一个 Job；用例按输入顺序串行运行，首个失败用例结束本次判题。
- C 源码以 `main.c` 编译一次，成功后将产物复制到各运行沙箱；Python 以 `main.py` 逐例启动解释器。
- 编译和每个用例分别使用新沙箱，应用 CPU、墙钟、进程树内存、进程数、FD、文件和磁盘配额。
- 输出比较先删除整个输出末尾的 ASCII 空白，再逐字节比较。
- 终态保存在内存中，调用方保存结果后 ACK；已确认结果按定长 FIFO 淘汰。

语言 ID 为 `c` 和 `python`。Python 使用部署提供的解释器与标准库，支持单文件提交。

## 安装与沙箱准备

运行需要 Linux、GCC、Python 3 和 Isolate。构建另需 Rust/Cargo 与支持 proto3 optional 的 `protoc`。

由管理员准备以下主机配置：

1. 以 root 所有且启用 setuid 的方式安装 Isolate，配置其 UID/GID 范围、沙箱根目录和控制组路径。
2. 为沙箱根目录所在文件系统启用用户空间及 inode 配额，并为各 box UID 保留相应容量。
3. 为每个运行中的 Worker 分配本机独占的 box ID。
4. 使用普通用户初始化时，在 `/etc/isolate` 设置 `restricted_init = 0`，并以运行 Worker 的用户检查配置：

   ```sh
   isolate --check-config
   ```

Worker 使用控制组模式；`/box`、`/tmp` 和阶段私有 `/dev/shm` 目录共享 box UID 的空间与 inode 配额。网络、环境变量和宿主文件可见范围见[隔离策略](../../docs/01-worker-design.md#72-基于-isolate-默认规则的隔离策略)。

### 用户配额文件系统示例

`restricted_init = 0` 允许普通用户初始化沙箱，`isolate --check-config` 检查配置有效性。磁盘空间和 inode 配额还要求工作目录所在文件系统启用用户配额；缺失时，实际判题会在初始化阶段报告 `Cannot set disk quota: quotas have not been enabled for this filesystem`。相关选项见 [Isolate 手册](https://github.com/ioi/isolate/blob/master/isolate.1.txt)。

以下采用 `/etc/isolate` 中默认的 `box_root = /var/lib/isolate`，由管理员为它创建一个新的 1 GiB ext4 镜像。先停止使用该目录的 Worker；镜像路径必须不存在，工作目录必须为空，脚本在条件不满足时停止。自定义 box_root 时相应调整目录。

```sh
sudo sh -eu <<'SH'
acorn_quota_image=/var/lib/acorn-isolate.img
test ! -e "$acorn_quota_image"
mkdir -p /var/lib/isolate
test -z "$(find /var/lib/isolate -mindepth 1 -maxdepth 1 -print -quit)"
truncate -s 1G "$acorn_quota_image"
mkfs.ext4 -O quota -E quotatype=usrquota "$acorn_quota_image"
mount -o loop "$acorn_quota_image" /var/lib/isolate
SH
```

ext4 的 quota 特性在创建文件系统时建立用户配额元数据，并在挂载时自动启用，见 [ext4 手册](https://man7.org/linux/man-pages/man5/ext4.5.html)。镜像容量是整个沙箱文件系统的容量，各 box 的空间和 inode 上限仍由 Worker 配置决定。

主机或 WSL 重启后，若挂载未保留，重新挂载已有镜像：

```sh
sudo mount -o loop /var/lib/acorn-isolate.img /var/lib/isolate
```

重新挂载使用已有文件系统，不再次创建或格式化镜像。需要验证真实判题时使用[既有验收命令](../backend/README.md#检查命令)，不把配置语法检查当作执行验收。

## 启动

在仓库根目录执行：

```sh
cargo build -p acorn-worker --locked
mkdir -p "$HOME/.config/acorn"
cp config/worker.example.toml "$HOME/.config/acorn/worker.toml"
```

编辑配置中的工具路径、监听地址和 `sandbox.box_id` 后启动：

```sh
target/debug/acorn-worker --config "$HOME/.config/acorn/worker.toml"
```

启动时校验 TOML 并检查工具可调用性，随后监听 gRPC。命令行支持 `--help`、`--version`。通过后端提交源码的完整流程见[快速开始](../../README_zh-CN.md#quick-start)。

## 配置

完整字段见 [worker.example.toml](../../config/worker.example.toml)。配置在启动时读取，修改后重启生效。

| 配置节             | 主要设置                                                   | 示例值                                     |
| ------------------ | ---------------------------------------------------------- | ------------------------------------------ |
| `server`           | gRPC 监听及消息上限                                        | `127.0.0.1:50051`，请求 16 MiB、响应 1 MiB |
| `request`          | ID、源码、用例数、输入、答案和总载荷上限                   | 源码 64 KiB、最多 128 例、总载荷 8 MiB     |
| `execution`        | Job 总期限、单用例资源上限、进程数及输出保留               | Job 120 秒，运行进程数 1                   |
| `languages.c`      | GCC 路径和编译参数                                         | `/usr/bin/gcc`，C17、`-O2`                 |
| `languages.python` | Python 路径和运行参数                                      | `/usr/bin/python3`，`-I -B -S main.py`     |
| `compile`          | 编译资源、诊断和导出产物预算                               | CPU 10 秒、墙钟 20 秒、内存 512 MiB        |
| `sandbox`          | Isolate 路径、box ID、FD、文件／磁盘／inode 配额和收尾超时 | 文件 16 MiB，磁盘 64 MiB，4096 inode       |
| `results`          | 未确认结果容量、已确认结果保留数量                         | 各 128 条                                  |
| `logging`          | stderr 日志级别                                            | `info`                                     |

请求中的单用例限额受 Worker 配置上限约束。内存、单文件和磁盘空间配置使用 KiB；源码、输出、消息和编译产物预算使用字节；时间使用毫秒。部署多个节点时，将各 Worker 预算与[后端配置](../backend/README.md#配置)对齐。

### Kafka 客户端配置

构建默认动态链接系统 librdkafka（至少 2.12.1），需要开发包及 pkg-config；静态构建方法见[根说明](../../README_zh-CN.md#构建)。仅构建 Worker 时可执行 `cargo build -p acorn-worker --locked --no-default-features --features kafka-static`。

`[kafka]` 必填，必须显式提供 `bootstrap_servers`、`tasks_topic`、`events_topic` 和 `group_id`。端点使用 `host:port` 或 `[IPv6]:port`，不带 URL 协议；两个 topic 必须不同。消息上限及 `delivery_timeout_ms`、`queue_timeout_ms`、`commit_timeout_ms`、`shutdown_timeout_ms` 的默认值与范围见[后端配置](../backend/README.md#kafka-客户端配置)，两端按部署对齐。使用已有配置时，将 `[kafka]` 节合并到实际 `--config` 指定的文件，保留工具路径、沙箱和执行配置。

配置直接使用 [acorn-kafka](../kafka/README.md) 的普通 `KafkaConfig`。`kafka::producer_config(&config.kafka)`／`consumer_config(&config.kafka)` 分别选择事件生产／任务消费，并调用共享客户端策略，采用 PLAINTEXT、幂等生产及 `acks=all`、手动位点、`auto.offset.reset=error`、classic 消费组，两者均禁止自动创建 topic。传输开销和队列软限制沿用[后端配置说明](../backend/README.md#kafka-客户端配置)，但发送预算对应事件、接收预算对应任务。

既有 Worker 配置的 `[kafka].tasks_group_id` 改名为 `group_id`，保留原任务消费组值。

当前服务入口使用 gRPC 判题。Kafka 消息编解码接口见[共享协议](../protocol/README.md#kafka-消息编解码)。

库接口 `kafka::KafkaWorker::new(worker, producer)` 从 `rpc::WorkerService` 取得同一配置及执行入口，`run(stop)` 接收 `watch::Receiver<bool>` 并返回可监督的服务结果。调用方先按同一配置通过 `kafka::producer_config(&config.kafka).create()` 构造事件生产者，并保留 `WorkerService` 供状态 RPC 使用，且仅选择一种任务接入路径。正式消费组位点由部署初始化；服务不自动重置。

Kafka 服务保持本机单槽，执行期间持续驱动消费组，忙或结果名额不足时暂停分区。执行前预留结果容量，同一 Job 的待发进度只保留最新快照；终态按 FIFO 发布，确认后只重试原任务位点。失去原分区归属的执行仍可回传终态，但不提交旧位点。BLOCKED 后立即开始关闭任务消费者并退出组，状态 RPC 和结果生产者继续运行；正常停机通过既有停止通道收尾执行，再以 `kafka.shutdown_timeout_ms` 限制全部结果及位点的共同消息收尾。

任务消费沿用 StreamConsumer，退出组由 SDK RAII 完成。释放前失效原归属，等待在途提交释放消费者引用，再在阻塞线程销毁最后一个句柄；SDK 关闭错误由 SDK 日志报告。共同消息预算同时约束等待消费者释放，超时会报告未完成收尾，但不会取消已经开始的析构，进程退出仍可能等待它。

## gRPC 接口

服务名为 `acorn.worker.v1.JudgeWorker`，定义见 [worker.proto](../../proto/acorn/worker/v1/worker.proto)。Rust 类型和调用入口由 [acorn-protocol](../protocol/README.md)提供。

| RPC                 | 用途                                              |
| ------------------- | ------------------------------------------------- |
| `SubmitJob`         | 提交 ID、语言、源码、用例和限额，立即返回接纳状态 |
| `GetJob`            | 查询阶段、已完成用例数或终态                      |
| `AcknowledgeResult` | 确认结果已保存，使记录进入可淘汰 FIFO             |
| `GetWorkerStatus`   | 查询执行占用、停机标记、未确认结果数量和容量      |

忙时、结果区满或停机时拒绝新任务，由后端管理排队。同 ID 的保留记录直接返回；结果被淘汰或进程重启后，该记录随之消失。Worker 的提交执行独立于 RPC 连接，调用方断开后继续执行。

判题结论包括 AC、WA、CE、RE、TLE、MLE、OLE；基础设施错误返回独立的失败终态。CE 带编译诊断，用例失败带首个失败用例的 stdout/stderr、截断标记和可用资源统计。Python 语法错误在用例运行阶段形成 RE。

## 日志与停机

英文日志输出到 stderr，`logging.level = "debug"` 可查看轮询和 ACK。Job ID 可用于与后端日志关联。

Ctrl-C 或 SIGTERM 先停止接纳，再停止当前执行并完成共同清理流程。`sandbox.shutdown_timeout_ms` 控制停机等待预算。正常退出码为 0，启动、监听或停机错误为 1。

停止无法确认时，Worker 保留执行占用并拒绝新任务，继续提供结果查询和 ACK；部署端根据错误日志处置。结果保存在本进程内存中，重启后由后端保留已落库的业务结果。

## 检查命令

在仓库根目录执行常规测试：

```sh
cargo test -p acorn-worker --locked
```

真实 Kafka 的定向检查只使用临时 topic 和消费组，需要可创建／删除测试资源的 Kafka 入口：

```sh
ACORN_TEST_KAFKA_SERVERS=localhost:9092 \
  cargo test -p acorn-worker --locked --test kafka real_kafka_worker_capacity_and_shutdown -- --ignored
ACORN_TEST_KAFKA_SERVERS=localhost:9092 \
  cargo test -p acorn-worker --locked --test kafka real_kafka_blocked_worker_leaves_group_for_replacement -- --ignored
ACORN_TEST_KAFKA_SERVERS=localhost:9092 \
  cargo test -p acorn-worker --locked --test kafka real_kafka_result_backpressure_and_total_shutdown_budget -- --ignored
```

这些用例复用受控 Isolate 管理器，检查消费、分区撤销、结果交接和停机，不作为隔离证据。一条真实 Isolate 判题另需空闲的专用 box ID：

```sh
ACORN_TEST_KAFKA_SERVERS=localhost:9092 ACORN_ISOLATE_BOX_ID=210 \
  cargo test -p acorn-worker --locked --test kafka real_kafka_isolate_execution -- --ignored
```

### Isolate 阶段检查

在已配置沙箱的主机上，使用空闲的专用 box ID 检查编译、运行、隔离和配额：

```sh
ACORN_ISOLATE_BOX_ID=200 ACORN_ISOLATION_SENTINEL=host-only-value \
  cargo test -p acorn-worker --locked --test isolate_real -- --ignored --nocapture
```

阶段测试按默认 `/var/lib/isolate` 根目录检查清理。执行器和 RPC 的真实测试分别使用：

```sh
ACORN_ISOLATE_BOX_ID=201 \
  cargo test -p acorn-worker --locked --test executor -- --ignored --nocapture
ACORN_ISOLATE_BOX_ID=202 \
  cargo test -p acorn-worker --locked --test rpc -- --ignored --nocapture
```

完整协议、资源规则和失败处理见 [Worker 设计](../../docs/01-worker-design.md)。

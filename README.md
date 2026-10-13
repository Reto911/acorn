# Acorn

C 与 Python 3 的单文件源码判题系统。后端包含 HTTP API、Scheduler 和 SQLite 访问模块，通过 gRPC 调用独立 Worker。

当前已完成 P01–P10：Worker 独立提供 C/Python 串行判题，后端通过 HTTP 创建提交、进程内 Scheduler 分派、gRPC 取结果、SQLite 持久化和 ACK 完成业务闭环，并已接入启动装入、正常停机及故障退出。首版源码样例、真实单／双 Worker 验收、独立程序从空库启动及重启查询已完成，操作步骤见下文。开发范围、执行契约和实际验证记录见[文档索引](docs/README.md)与[开发计划](docs/04-development-plan.md)。

浏览器 GUI 采用 Vue 3、TypeScript 和 Monaco，已完成 F01–F07：题目浏览、C/Python 编辑、本地草稿、主动提交、结果详情、固定版本题面、状态轮询，以及提交历史筛选、分页和详情往返。首版桌面联调已覆盖 C/Python AC、C CE、Python WA 和 C TLE，实际新增与复用记录、UI-01–UI-09 对应关系及未测范围见[前端开发计划](docs/06-frontend-development-plan.md#57-f07首版浏览器联调与操作说明)。交互依据见[GUI 设计](docs/05-frontend-design.md)；健康观测 F08 / H01–H04 已完成，提供 Worker 状态 RPC、后端共享采集、只读 HTTP 快照及 `/health` 系统状态页面，支持自动、手动刷新和独立过期警告；初次实际检查见[健康页面交付记录](docs/06-frontend-development-plan.md#511-f08h04健康页面与最终连通验收)，后续每 10 秒自动获取的实现与验证边界见[补充记录](docs/06-frontend-development-plan.md#512-健康页面每十秒自动获取)。

LLM 代码审查正在实施，合同与 L01–L06 计划见[LLM 设计](docs/08-llm-code-review-design.md)。L01 已完成数据类型、结构版本 3、最新审查读写与历史筛选的数据库基础；L02 已完成配置、固定策略和单次 Responses 客户端；L03 已接通独立有界队列、判题后自动审查、启动中断及停机取消，见[队列与生命周期实施记录](docs/08-llm-code-review-design.md#123-l03队列自动触发与生命周期)。配置 enabled=true 后仍须题目固定版本的 review_mode 达到自动门槛，默认题目策略为 DISABLED；机器结果保存、ACK 和 Worker 本地占用释放后才交接审查，模型等待不占数据库连接。手动触发、HTTP 审查查询和 GUI 仍待 L04–L06。当前后端只接受结构版本 3；已有版本 2 数据库须按[离线升级交付](docs/08-llm-code-review-design.md#1212-离线升级交付)停服务、备份并单独执行升级 SQL，不能重跑初始化 SQL。既有判题和健康验收不扩展为完整 LLM 验收。

## 浏览器 GUI

先按下文[程序入口](#程序入口)启动 Worker 和后端；已有数据库只需使用原配置启动，不重复执行初始化 SQL。再按[前端环境与安装](web/README.md#环境与安装)准备 Node.js、pnpm、依赖和本地配置，在仓库根目录执行：

```sh
cd web
pnpm dev
```

打开 [本地题目列表](http://localhost:5173/problems)，进入两数求和题，选择 C 或 Python 3，粘贴对应的 `examples/submissions/a-plus-b.c` 或 `a-plus-b.py` 源码。Ctrl+S 保存本地草稿，点击“提交代码”后进入结果页并取得 AC；“本题提交记录”可重新打开结果。完整操作、检查和静态部署说明见[前端说明](web/README.md)。GUI 支持宽度不小于 1024 CSS px 的桌面浏览器；可从“系统状态”导航或 [本地健康页面](http://localhost:5173/health)查看获取时快照并自动刷新，具体说明见[健康观测操作](web/README.md#健康观测)。当前没有登录或历史源码查询。

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
cargo build --workspace --locked
cp config/worker.example.toml /tmp/acorn-worker.toml
```

真实运行需要 Linux、Isolate、GCC、Python 3、控制组及沙箱文件系统用户配额，准备方式见下文 [Isolate 阶段验收](#isolate-阶段验收)。初始化数据库和发送请求另需 `sqlite3`（3.38.0 或更新版本）及 `curl`。编辑 `/tmp/acorn-worker.toml`，填写实际工具路径，并将 `sandbox.box_id` 设为本机专用于此次运行的空闲 ID，例如 210。每个运行中的 Worker 独占一个 ID。

在仓库根目录的终端 A 启动 Worker，保留英文日志：

```sh
target/debug/acorn-worker --config /tmp/acorn-worker.toml 2>/tmp/acorn-worker.log
```

有效 Worker 配置通过校验后启动 gRPC 监听，提供 `SubmitJob`、`GetJob`、`AcknowledgeResult` 和 `GetWorkerStatus`。提交立即返回，后台执行不随提交连接断开而取消；一个 Worker 同时只执行一个 Job，忙时不排队。终态须由调用方保存后 ACK，未确认结果区满时拒绝新任务。请求和结果消息上限从配置应用到 Tonic 服务端。`GetWorkerStatus` 只读返回执行状态、停机标记和未确认结果数量及容量，忙时也可查询；该观测不参与调度和接纳，契约与实施范围见[健康观测设计](docs/07-health-observation-design.md)。

Ctrl-C 或 SIGTERM 先停止接纳，再通过共同停止路径结束当前执行，按 `sandbox.shutdown_timeout_ms` 等待收尾。正常停机返回 0；启动、监听、停机超时或停止未确认明确报错并返回 1。停止未确认时，运行中的服务仍可查询和 ACK 失败结果，但保持执行位置占用，由部署端处置。结果只存在本进程内存，重启不恢复。

无效配置或不可调用的工具明确报错，不启动监听。工具的 `--version` 检查不能替代真实沙箱验收。

后端读取 [backend.example.toml](config/backend.example.toml)，初始化英文日志，打开已初始化数据库并检查结构版本及预置测试用户，然后启动独立审查服务、Scheduler、共享健康采集和 HTTP 监听。数据库路径相对于启动目录；示例使用 `acorn.db`、用户 1 和 `http://127.0.0.1:50051` Worker endpoint。先启动上文的 Worker，再在另一终端于仓库根目录准备新数据库并启动后端：

```sh
sqlite3 -bail acorn.db < sql/001-initial-schema.sql
sqlite3 -bail acorn.db < examples/seed-user.sql
sqlite3 -bail acorn.db < sql/002-example-problem.sql
cp config/backend.example.toml /tmp/acorn-backend.toml
target/debug/acorn-backend --config /tmp/acorn-backend.toml 2>/tmp/acorn-backend.log
```

三个 SQL 脚本仅在新库执行一次。应用不会创建数据库、装入 SQL 或执行升级。测试用户脚本仅供开发，Argon2id 哈希来自已丢弃的随机密码；当前没有登录功能或可用登录凭据。示例题仍随正式发布保留。

后端在开放 HTTP 前将旧 `dispatching/running` 保存为 `failed / SchedulerInterrupted`，将旧审查 PENDING 保存为 FAILED/ReviewInterrupted，按 `created_at_ms, id` 装入 `queued` 的固定版本任务；排队数超出 `scheduler.pending_max_count` 时明确报错退出，提示部署端增大配置上限。启动不联系旧任务的 Worker，也不补发旧 ACK 或模型请求。

后端收到 Ctrl-C 或 SIGTERM 后停止创建接纳和新分派，同时停止审查接纳、取消本地模型等待并按 ID 保存 FAILED/ReviewInterrupted；判题收尾时满足自动门槛的新终态直接原子保存审查中断。等待已开始的短创建操作、在途判题、结果保存、一次有 deadline 的 ACK 及本地审查收尾，再关闭 HTTP，停止并等待健康采集任务，最后关闭连接池。等待期间查询继续可用，新创建返回 `503 BackendStopping`；未分派任务保持 `queued`，下次启动装入。已开始的创建操作即使尚在幂等查询阶段，也可在正常停机时完成名额预留与本地交接。

运行期普通业务只读失败返回 `500 InternalError` 并记录英文日志，后端继续运行。健康读取探测的普通失败只更新观测缓存，健康接口仍可返回 `200`。创建写事务及状态、终态写入失败，明确的 I/O、磁盘满或数据库损坏，以及 Scheduler 异常结束，会使后端停止接纳与分派，收尾短创建操作后尽快退出，不等待其他 Worker 完成；未保存结果不 ACK。具体边界见[后端故障契约](docs/03-backend-design.md#83-数据库及后台故障)。

查询题目并提交 Python 两数求和源码：

```sh
curl http://127.0.0.1:8080/api/v1/problems
curl -i http://127.0.0.1:8080/api/v1/submissions \
  -H 'Content-Type: application/json' \
  -H 'Idempotency-Key: 7b5f28d3-51a4-463c-98a4-0a11815d8d46' \
  -d '{"problem_id":1,"language_id":"python","source":"a,b=map(int,input().split());print(a+b)\n"}'
```

创建返回 `202`、`submission_id`、`state` 和 `Location`；同键同内容重传返回 `200`，新的主动提交更换 UUID 键。按返回的 `Location` 轮询详情即可取得持久化终态；提交历史为 `GET /api/v1/submissions`。两个列表都返回 `{"items":[...],"limit":20,"offset":0}`，支持 `limit`、`offset`；详情中的终态字节字段使用 Base64。业务路由、健康接口、筛选和英文错误码见[HTTP 契约](docs/03-backend-design.md#5-http-接口)。

后端启动后可查询只读健康快照：

```sh
curl -i http://127.0.0.1:8080/api/v1/health
```

该接口返回本地接纳条件、Scheduler 生命周期及名额、数据库与 Worker 最近观测，附带 `Cache-Control: no-store`；请求不触发新探测。默认每轮结束后等待 5 秒，单项预算 1 秒，观测年龄达到 12 秒后显示 `stale`。Worker 失败或数据库普通读取失败时仍返回组件快照，不代表整体可用或保证下一次提交成功。采集任务异常结束时只记录错误，判题服务继续，旧观测随后过期；字段与错误码见[健康契约](docs/07-health-observation-design.md#5-http-契约与可见范围)。

### 从源码文件提交并核对结果

以下文件均用于题号 1、版本 1 的两数求和题；错误和超限样例是有意构造的判题输入：

| 源码 | `language_id` | 预期结果 |
| --- | --- | --- |
| [a-plus-b.c](examples/submissions/a-plus-b.c) | `c` | `finished / AC`， 104 个用例通过 |
| [a-plus-b.py](examples/submissions/a-plus-b.py) | `python` | `finished / AC`，新库的 104 个用例通过 |
| [compile-error.c](examples/submissions/compile-error.c) | `c` | `finished / CE`，有编译诊断，不运行用例 |
| [wrong-answer.py](examples/submissions/wrong-answer.py) | `python` | `finished / WA`，失败用例序号为 1，stdout 解码为 `0\n` |
| [time-limit.c](examples/submissions/time-limit.c) | `c` | `finished / TLE`，失败用例序号为 1 |

在终端 C 从 C 文件生成请求 JSON。Python 标准库负责保留源码换行和 JSON 转义：

```sh
python3 - c examples/submissions/a-plus-b.c > /tmp/acorn-request.json <<'PY'
import json
from pathlib import Path
import sys

print(json.dumps({
    "problem_id": 1,
    "language_id": sys.argv[1],
    "source": Path(sys.argv[2]).read_text(encoding="utf-8"),
}))
PY
ACORN_REQUEST_KEY=$(python3 -c 'import uuid; print(uuid.uuid4())')
curl --fail-with-body --silent --show-error \
  http://127.0.0.1:8080/api/v1/submissions \
  -H 'Content-Type: application/json' \
  -H "Idempotency-Key: $ACORN_REQUEST_KEY" \
  --data-binary @/tmp/acorn-request.json \
  --dump-header /tmp/acorn-create.headers \
  --output /tmp/acorn-create.json
cat /tmp/acorn-create.json
```

请求成功后按响应的 `Location` 轮询，最多等待 60 秒：

```sh
python3 - <<'PY'
import json
from pathlib import Path
import time
from urllib.request import urlopen

headers = Path("/tmp/acorn-create.headers").read_text().splitlines()
location = next(line.split(":", 1)[1].strip() for line in headers
                if line.lower().startswith("location:"))
deadline = time.monotonic() + 60
while time.monotonic() < deadline:
    with urlopen("http://127.0.0.1:8080" + location, timeout=5) as response:
        result = json.load(response)
    if result["state"] in ("finished", "failed"):
        text = json.dumps(result, ensure_ascii=False, indent=2)
        Path("/tmp/acorn-result.json").write_text(text + "\n")
        print(text)
        break
    time.sleep(0.5)
else:
    raise SystemExit("Timed out waiting for a terminal submission")
PY
curl --fail-with-body --silent --show-error \
  'http://127.0.0.1:8080/api/v1/submissions?problem_id=1&limit=20'
```

对 Python 重复上述步骤，将生成请求的命令参数改为 `python examples/submissions/a-plus-b.py`，并生成新的键。C/Python 均应返回 `finished / AC`；保留两个 `submission_id` 供重启后查询。其他三个文件使用表中的语言和相同步骤提交。CE/WA/TLE 属于正常判题结论，状态为 `finished`；`failed` 表示执行或跟踪故障，应检查 `terminal_details` 和日志。CE 诊断、失败用例 stdout/stderr 使用标准 Base64；缺失资源统计保持缺失。

需要重传同一请求时，保留 `/tmp/acorn-request.json` 和原 `ACORN_REQUEST_KEY`，只重复 `curl`，预期返回 `200` 和原 Submission ID。主动重新判题时生成新键；相同键更换源码会返回 `409 IdempotencyConflict`。

### 双 Worker 配置、日志与停止

第二个 Worker 复制同一示例配置，修改 `server.listen` 为 `127.0.0.1:50052`，`sandbox.box_id` 为另一个专用 ID，例如 211。后端 `/tmp/acorn-backend.toml` 中的 `[scheduler]` 使用：

```toml
workers = ["http://127.0.0.1:50051", "http://127.0.0.1:50052"]
```

两个 Worker 采用相同载荷、输出和资源门限；后端配置的最大任务／结果不得超出任一节点的能力。先启动两个 Worker，再启动后端。两个提交可分布执行；每个 Worker 同时只执行一个 Job，Job 内编译和各用例串行。真实忙拒绝与并行执行的自动验收命令见下文。

按 `submission_id` 搜索 `/tmp/acorn-backend.log` 中的 `Submission saved`、`Dispatching submission` 和 `Submission terminal committed`，可关联题号、固定版本、语言及 Worker 地址。首版 Submission ID 就是 Worker Job ID；在对应 Worker 日志中可找到 `Job accepted`、`Case started` 和 `Judging finished`。普通轮询和 ACK 日志为 debug，需要查看时在两端配置中将 `logging.level` 改为 `debug` 后启动。日志不包含完整源码或测试数据。

正常停止时，先在后端终端按 Ctrl-C，等待后端完成在途保存、一次 ACK 并退出，再停止 Worker。也可由服务管理器发送 SIGTERM。重新运行同一后端配置，使用 `GET /api/v1/submissions/<submission_id>` 查询此前两个 AC；历史终态仍应存在。数据库初始化 SQL 不重复执行。未分派任务保留排队状态，旧在途任务按前述重启规则中断。

内存限额和 Isolate 单文件／磁盘空间限额统一使用 KiB（1 KiB = 1024 字节），配置字段明确以 `_kib` 结尾；实际资源统计、输出保留和产物导出大小仍按字节计量。数据库结构版本为 2，内存列为 `memory_kib`；旧配置与旧数据库需更新，后端与 Worker 按当前 KiB 协议同步构建运行。已有数据库由部署端离线转换或另建新库，详见 [SQL 使用说明](sql/README.md)。

P06–P09 的数据库、Scheduler、HTTP 和进程生命周期常规测试：

```sh
cargo test -p acorn-backend --locked
```

真实单 Worker 验收读取上述五份源码，通过实际 HTTP 端口、Scheduler、gRPC、SQLite 和 Isolate 检查 C/Python AC、C CE、Python 首例 WA、C TLE、持久化详情，以及保存与 ACK 淘汰。使用独立 box ID，在具备既定权限的宿主环境执行：

```sh
ACORN_ISOLATE_BOX_ID=204 \
  cargo test -p acorn-backend --locked --test workflow real_http_worker_acceptance -- --ignored --nocapture
```

真实双 Worker 验收需要两个不同的专用 box ID。它验证两个 HTTP 提交在不同节点同时执行，以及后端未知的占用造成明确忙拒绝后立即转交另一节点；使用 60 秒重试间隔和有界等待区分立即转交与定时重试，不作毫秒级性能断言：

```sh
ACORN_ISOLATE_BOX_ID=205 ACORN_ISOLATE_BOX_ID_2=206 \
  cargo test -p acorn-backend --locked --test workflow real_two_workers_acceptance -- --ignored --nocapture
```

这两个测试在测试进程中组装真实服务；上文从新库启动独立程序的操作流程另外验收。它们不会配置系统配额或修改 Isolate 安装，环境前提未满足时明确失败。真实验收的实际结果和范围记录在[开发计划](docs/04-development-plan.md#7-实施记录)。

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

常规 RPC 测试也会绑定回环随机端口，但使用受控管理器，不要求 Isolate 初始化权限。限制本机 socket 的工具沙箱需在宿主执行这些测试；受控管理器的 GCC/Python 执行仅验证编排，不验证真实隔离。

## Isolate 阶段验收

部署时保留 Isolate 官方的 root 所有权和 setuid 安装方式，在沙箱根目录所在文件系统启用用户配额，并准备控制组。Worker 仍以启动用户运行。若采用普通用户初始化方式，由管理员在 `/etc/isolate` 设置 `restricted_init = 0`，再以该用户执行 `isolate --check-config`；配置加载成功仍不能替代初始化与实际运行检查。隔离和配额约定见 [Worker 设计第 7.2 节](docs/01-worker-design.md#72-基于-isolate-默认规则的隔离策略)。

默认测试运行无需 Isolate 初始化权限；真实验收单独执行，须选择专用 box ID，避免与其他 Worker 冲突。以下命令使用 box 200，测试按本机默认 `/var/lib/isolate` 根目录检查清理：

```sh
ACORN_ISOLATE_BOX_ID=200 ACORN_ISOLATION_SENTINEL=host-only-value \
  cargo test -p acorn-worker --locked --test isolate_real -- --ignored --nocapture
```

该测试实际编译 C、使用独立程序副本执行两次并运行 Python，检查输出超限停止、1 KiB 单文件上限触发 `EFBIG`、三处可写目录共享空间及 inode 配额，以及宿主私有文件、继承环境、额外 FD 和宿主监听端口的隔离。配额设置失败、管理器异常退出和清理警告另由可控测试覆盖；停止未确认时跳过依赖停止的清理并保留执行占用，需由部署端处置。

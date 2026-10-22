# LLM 代码审查实施记录

对应设计：[LLM 代码审查设计](../08-llm-code-review-design.md)。历史数据库交付约定见[方案记录](../d01-review/08-llm-code-review-review.md)。

当前状态：L01–L06 已完成，自动与手动接纳、独立队列、生命周期、查询和 GUI 已接入；有限联调及一次完整审查链路的真实 API 检查已通过。

以下保留原设计第 12 节的章节编号、SQLite 版本 3 阶段的实际过程、失败与验证范围。MySQL 版本 4 的适配、真实判题和受控审查展示另见[迁移实施记录](09-mysql-migration-execution.md)，原阶段结果不扩展为 MySQL 或后续调用的验证。

## 12. 实施记录

### 12.1 L01：数据契约与离线升级

L01 已完成。[审查数据类型](../../crates/backend/src/llm_review.rs)定义三个模式、五个状态、两种意见、三个疑点类别、最新一次详情及稳定失败码。意见严格解析必填字段、类型、枚举及未知字段；校验闭区间行号、PASS/DOUBTFUL 与 findings 的关系及 UTF-8 内容预算。固定可空键序列化为 null，缺失用量不估算，错误说明使用后端固定英文文本。

详情组装与数据库写入均检查 32 KiB 总预算，意见和可选诊断另受 16 KiB 上限约束。接纳元数据还须容纳固定失败记录；有效意见或响应元数据造成总详情超限时组装 FAILED/ReviewInvalidOutput，移除超限的 response_id，诊断不能完整保留时省略，不截断 JSON。无效数据在应用边界明确报错，不作为 SQL 存储故障通知。

[完整初始 SQL](../../sql/001-initial-schema.sql)与[后端版本检查](../../crates/backend/src/database/mod.rs)已切换至结构版本 3，增加四列及枚举、JSON 对象和列间空值约束，保留既有机器终态约束和索引。SQL 约束 DISABLED 的 ID/详情为空、其他状态仅出现在 finished 且 ID/详情非空；JSON 内的状态关系、固定错误、行范围和内容预算由应用检查。不新增审查表、索引或自动迁移。

[数据库审查操作](../../crates/backend/src/database/reviews.rs)提供固定源码、语言、题面和 verdict 的读取，最新 PENDING 的条件替换，按 review_id 完成，以及显式中断旧 PENDING。重跑覆盖审查字段；旧 ID、重复完成或已有 PENDING 不覆盖当前尝试。完成时按实际保存源码的 LF 物理行数校验行范围，保留 CRLF 字节，末尾换行不额外计作空行。读取返回后已释放连接。中断操作保留 ID 和请求元数据、补完成时间，入口接线留在 L03。

[提交数据库操作](../../crates/backend/src/database/submissions.rs)已支持机器终态与自动审查初态同一条件 UPDATE 原子保存，仍返回更新是否生效；现有 Scheduler 显式传入无审查初态，继续保存 DISABLED。JudgingTask 从共同装配方法携带固定版本 review_mode，不加入 Worker 请求。题目和提交内部类型、数据库历史的 review_status AND 筛选已扩展；当前 HTTP 仍使用原 DTO 和筛选，审查字段及手动服务留在 L04，前端留在 L05。

#### 12.1.1 实际检查

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过，包含全部现有 Rust 测试目标；未新增依赖。
- `cargo test -p acorn-backend --locked --lib`：八项通过，新增两项意见与详情预算检查，覆盖未知字段、必填值、闭区间、中文 UTF-8 字节、JSON 转义开销、无效意见、总详情超限及诊断省略。
- `cargo test -p acorn-backend --locked --test database`：在可绑定回环端口的宿主环境九项通过。新增两项覆盖默认 DISABLED、固定版本策略和输入、PENDING/PASS/DOUBTFUL/FAILED 往返、条件重跑及迟到结果、实际源码行范围、启动中断方法、机器结果保留、组合筛选、SQL 约束和写入预算。现有独立后端启动测试通过。
- `cargo test -p acorn-backend --locked --test workflow`：在宿主环境二十四项通过，两个真实 Isolate 验收按原规则忽略。现有 HTTP、保存后 ACK、启动停机和健康流程继续通过；新审查队列、HTTP 接纳或生命周期尚无实现，不据此声称已验收。
- 通过 `sqlite3 -bail` 对临时版本 2 数据库执行升级 SQL：七条 queued、dispatching、running、AC、CE、WA 和 failed 记录的原字段完整保留，新增字段默认 DISABLED/null，结构版本为 3。新库与升级库的列、外键、索引及去除空白后的完整结构定义一致，`foreign_key_check` 为空；错误版本执行在修改列之前拒绝。
- 变更 Markdown 的本地链接与锚点、代码块闭合、既有标题编号保留，以及 `git diff --check`：通过。

本阶段仅验证临时数据库；未运行完整 workspace 测试、真实 Isolate 或浏览器验收。L01 不包含客户端调度、HTTP 和前端接入。

#### 12.1.2 离线升级交付

一次性升级 SQL 已单独交付，不加入仓库，也不由应用加载。该文件以一个事务添加四列，检查原 user_version 必须为 2，成功后才设置为 3；旧记录不补跑审查。

部署方停止后端后，将以下数据库、备份和升级文件占位路径替换成实际路径，先确认原版本为 2、完成备份，再执行升级：

```sh
sqlite3 -bail /path/to/acorn.db 'PRAGMA user_version;'
sqlite3 -bail /path/to/acorn.db ".backup '/path/to/acorn-v2-backup.db'"
sqlite3 -bail /path/to/acorn.db < /path/to/upgrade.sql
sqlite3 -bail /path/to/acorn.db 'PRAGMA user_version; PRAGMA foreign_key_check;'
```

最后一条应只输出版本号 3，外键检查无记录，再使用原 TOML 启动新版后端。版本 1 不适用此脚本；已有库不执行完整初始化 SQL，升级失败先按原错误处理，不继续启动或只修改版本号。本阶段仅验证临时库，没有代为执行实际部署升级。

### 12.2 L02：审查客户端与策略

L02 已完成。[后端配置](../../crates/backend/src/config.rs)和[示例](../../config/backend.example.toml)增加可缺省的 llm_review，旧配置保持禁用，部分配置沿用已批准的默认值。启用时检查完整 HTTP(S) URL、非空密钥与模型、none/low/high/max 和正值预算，并复用 L01 的详情构造验证接纳元数据及固定失败记录预算；不发送启动收费探测。配置 Debug 隐藏密钥和 endpoint，TOML 解析错误只保留字节位置，不回显可能含密钥的配置原文。没有 .env 正式加载逻辑、权限位检查或 dotenv 依赖。

[固定策略](../../crates/backend/src/llm_review/policy.rs)维护 review-policy-v1 指令及 acorn_code_review Schema。只发送完整源码、语言、固定公开标题和题面及机器 verdict，源码在嵌套 JSON 解码后逐字节保持原样，不预处理或剥离注释。固定 instructions 覆盖[第 6.1 节](../08-llm-code-review-design.md#61-输入)的反作弊范围、说明性注释与实际语言语义的区别、闭区间行号及正常解释要求。Schema 约束字段、枚举和对象形状；UTF-8 字节预算及跨字段规则继续复用 L01 的类型校验，未新增通用 Schema 校验依赖。

[单次客户端](../../crates/backend/src/llm_review/client.rs)复用 reqwest 0.13.5 连接池，使用敏感 Bearer header、完整配置 URL、stream=false、store=false 和固定 json_schema，不设置 strict 或其他输出模式。显式关闭 reqwest 协议重试和 HTTP 重定向；每次调用只发送一次 POST。完整请求先序列化并检查实际字节；HTTP 总期限覆盖发送和完整读取，响应逐 chunk 检查上限，超限即停止，不保留部分正文。

解析按顺序拼接 assistant output_text，忽略 reasoning item，不假定首项是正文。仅 completed、无 API 错误或不完整标记且通过类型与业务校验的对象成为有效意见；工具、拒绝、空正文和无效 JSON 均失败。完整模型 JSON 对象可按 L01 预算作为诊断保留，即使响应 incomplete；API 错误正文和完整 Responses 外层对象不进入诊断。Responses 失败时仍保留实际返回的响应 ID 和有效用量；缺失计数保持 null，不推算。英文应用日志只记录约定身份、模型、耗时、HTTP 状态、结果和实际用量，不记录源码、指令、意见正文、诊断或推理内容。

客户端不持有数据库或 Scheduler 句柄：调用方先读取固定输入并释放连接，客户端返回可持久化 ReviewAttempt，之后由调用方按 ID 保存。当前程序入口尚未构造或调度客户端，启用配置不会自动启动审查。独立队列、自动门槛、启动中断与停止接线继续属于 L03，手动 HTTP 服务属于 L04。

#### 12.2.1 实际检查

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。Cargo.lock 已锁定 reqwest 0.13.5；正式后端只增加所需 HTTP/TLS 依赖，测试直接使用已存在于依赖图的 tokio-stream 构造受控响应流。
- `cargo test -p acorn-backend --locked --lib --test config_results`：八项现有单元测试、五项配置与结果测试通过。新增配置检查覆盖旧配置及部分缺省、启用校验、元数据预算、非法鉴权 header 和密钥不进入 Debug／TOML 错误链。
- `cargo test -p acorn-backend --locked --test llm_review`：在可绑定回环端口的宿主环境五项通过。使用临时 Axum 服务及假密钥，核对实际请求字段、源码字节和允许输入、有效 PASS/DOUBTFUL、reasoning 首项及多段正文、真实计数与显式零、无效与不完整输出、完整诊断保留、精确请求预算、响应在 EOF 前超限退出、响应读取超时及不重试／不跟随重定向。应用日志捕获中没有假密钥、源码、公开题面或推理内容。此类固定响应和指令文本检查不证明真实模型总能遵守反作弊规则。
- `cargo test -p acorn-backend --locked --test workflow`：宿主环境二十四项通过，两个真实 Isolate 验收按原规则忽略。默认禁用配置下的既有 HTTP、判题保存与 ACK、启动停机和健康观测继续通过。
- 变更 Markdown 的本地链接／锚点、代码块闭合及既有标题编号保留、shell 块语法和 `git diff --check`：通过。

#### 12.2.2 一次最小真实调用

按用户授权，使用根目录 .env 中的开发凭据，通过临时验证入口调用正式 ReviewClient。凭据只用于内存配置，正式程序、测试和构建过程均不读取 .env；验证后已移除临时入口。

本次仅发起一次请求，实际使用配置的 Responses endpoint、deepseek-flash、max、10240 token、json_schema 和 review-policy-v1，返回 HTTP 200、完整有效 PASS，摘要认为公开两数求和 Python 样例采用通用读取及求和逻辑。实际用量为输入 961 token、输出 85 token，其中推理 23 token；这些是提供商返回值，不是估算或费用承诺。

使用正式初始化、开发用户和示例题 SQL 建立临时版本 3 库，以 `examples/submissions/a-plus-b.py` 创建源码记录。机器 AC 由夹具直接构造，没有执行 Worker；手动审查初态、固定输入读取、真实客户端返回及按 ID 完成均通过正式数据库方法。最终 PASS、有效意见、响应 ID 和用量保存成功，原机器 verdict 与空 AC 终态详情保持不变。

本次 PASS 验证了单次真实调用和保存流程，不代表整体反作弊效果或后续调用必然得到相同意见。

本阶段没有接入自动或手动审查服务；未运行完整 workspace 测试、真实 Isolate 或浏览器验收。此次真实调用覆盖接口与保存流程，L03–L06 的队列、HTTP、UI 和最终联调仍待实施；下一步按 L03 实现独立队列、自动交接及程序生命周期。

### 12.3 L03：队列、自动触发与生命周期

L03 已完成。[审查服务](../../crates/backend/src/llm_review/service.rs)使用独立 FIFO、待执行信号量和受配置限制的运行任务，复用单个 ReviewClient 和既有数据库。预留及排队任务持有待执行名额，转入执行时释放；活动提交去重，任务只保留身份和请求元数据。模型输入在执行时读取，数据库方法返回后已归还连接；请求期间不占 Scheduler 名额、Worker 本地占用或数据库连接。按 review_id 条件完成，保留最近一次意见，不新增扫描补投、重试、恢复框架或依赖。

[自动策略](../../crates/backend/src/llm_review/policy.rs)复用 L01 已由共同任务装配入口携带的固定版本 review_mode，按已校验的机器终态判断 ANY、ALL 与 DISABLED；不修改 Worker 或 Proto。[判题跟踪](../../crates/backend/src/scheduler/tracking.rs)在终态保存前尝试预留独立审查名额，用既有条件 UPDATE 原子保存机器终态和审查初态。未生效或写入失败时释放名额，未保存的机器结果不 ACK；队列满、服务禁用或停止时直接保存相应审查失败。

首次机器终态保存后先执行一次 ACK，将审查意图和预留名额作为跟踪完成事件返回。[调度循环](../../crates/backend/src/scheduler/mod.rs)释放节点、路由及本地在途身份后再交接；ACK 失败仍交接，重复终态不新建审查。交接遇到停止或关闭时，先释放审查名额，再按本次 ID 保存 ReviewInterrupted 或 ReviewUnavailable；该写入发生在 ACK 和节点释放之后。正常交接只修改本地队列，不等待模型。

[程序入口](../../crates/backend/src/main.rs)在开放 HTTP 前调用旧 PENDING 中断操作，保留 ID 和请求元数据，不补发请求；启动并监督独立审查任务。停止信号立即关闭审查接纳并取消本地输入读取或模型等待，完成短写入和队列中断；判题收尾中新到的自动需求直接原子保存 FAILED/ReviewInterrupted。入口等待创建、判题及本地审查收尾后关闭 HTTP、健康采集和数据库。普通 API 失败只结束该次审查；关键审查写入失败或调度任务异常结束按既有故障规则退出，不等待远端模型执行或计费结束。

本阶段只接通自动审查。题目默认 DISABLED，enabled=true 仅启用审查服务，仍须提交固定版本满足门槛。手动接纳及短操作登记、HTTP DTO 和筛选留在 L04，前端及最终联调留在 L05–L06。

#### 12.3.1 实际检查

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：最终通过，覆盖程序入口、全部现有测试目标和更新后的真实 Worker 夹具。
- 沙箱执行 `cargo test -p acorn-backend --locked --lib --test llm_review --test workflow`：九项单元测试通过，新增自动门槛小表覆盖全部用例级 verdict 的首例／后续失败、AC、CE、执行失败及三种模式；五项客户端网络测试绑定回环端口时返回 Operation not permitted，命令失败，未执行 workflow。
- 随后在可绑定回环端口的宿主环境执行 `cargo test -p acorn-backend --locked --test llm_review --test workflow`，五项 L02 客户端测试通过。首轮新增 workflow 用例因测试误用 python3 而非既有 python 标识失败；修正夹具后运行 `cargo test -p acorn-backend --locked --test workflow`，并在补充条件终态未生效检查后再次运行，最终三十一项常规测试通过，两个真实 Isolate 验收按原规则忽略。
- [七项新增 workflow 检查](../../crates/backend/tests/cases/review_workflow.rs)使用临时 SQLite、受控 gRPC 和 Axum Responses 服务。覆盖单运行／单排队容量、FIFO、队列满、独立 Scheduler 容量、重复投递、迟到机器终态的名额释放、禁用服务及 ACK 失败。实际模型 POST 处理器断言 Scheduler 身份已释放且 ACK 已调用；卡住响应时仍能保存下一条机器终态并读取数据库。
- ACK 完成门控检查确认：机器结果和 PENDING 已落库、ACK 尚未结束时停止审查，PENDING 此时不被交接收尾覆盖；ACK 完成后保存 ReviewInterrupted，未发模型 POST。实际后端子进程另验证旧 PENDING 启动中断、排队任务使用旧版本策略、重启不补发、运行及排队审查停机中断、停机中新机器终态的原子中断，以及 HTTP 查询持续可用；60 秒模型预算内的未完成请求不阻止既有 5 秒进程退出检查。
- 受控 503 仅使该审查保存 ReviewApiFailed，后续审查正常完成。临时数据库触发器拒绝审查完成写入时，后端明确故障退出，机器 AC 和 ACK 保留，未写成的审查仍为 PENDING；没有把内存意见或失败记录伪称为已保存。
- 变更 Markdown 的本地链接与锚点、代码块闭合、既有标题编号保留，以及 `git diff --check`：通过。

未调用真实模型、读取开发凭据、升级实际部署数据库或运行完整 workspace、真实 Isolate、浏览器验收；未人工注入审查任务 panic。受控 PASS 验证调用和持久化合同，不证明模型语义效果。下一步 L04 实现手动接纳、预期 ID 校验、审查查询 DTO 和历史筛选。

### 12.4 L04：手动接口与查询

L04 已完成。[审查服务](../../crates/backend/src/llm_review/service.rs)登记并独立持有手动接纳短操作，通过一次性结果通道让 HTTP 等待响应；请求取消不会取消事务处理、交接或中断写入。队列任务监督这些操作并在停止时等待结束，现有入口的审查任务等待同时覆盖模型取消和短操作收尾。自动与手动使用同一个停止边界、排队名额和运行容量；活动去重改为按审查 ID，上一轮终态已落库而本地执行尚未清理时，新一轮仍可交接。

[数据库接纳](../../crates/backend/src/database/reviews.rs)在 BEGIN IMMEDIATE 短事务内统一判断不存在、非 finished、PENDING 复用、预期 ID 不符及容量不足；满足条件才替换最新 ID 和 PENDING。容量预留由审查服务持有，已有 PENDING 即使未取得名额也能复用；不接纳时保留旧审查。公共 start_review 复用相同事务流程，完成仍按 review_id 条件写入。事务提交并释放连接后才交接，没有修改结构版本或机器终态合同。

[HTTP 路由](../../crates/backend/src/http/mod.rs)已提供 POST review、稳定业务错误映射及 200／202 和 Location。expected_review_id 必填可空，UUID 规范化后交给服务；HTTP 不持有接纳执行的生命周期。题目最新／指定版本详情返回 review_mode，提交详情返回 review_status 和显式公共 review，历史摘要及筛选支持五个审查状态。公开字段按[第 8.1 节](../08-llm-code-review-design.md#81-接口)固定七键，缺失值保持 null；响应 ID、用量、策略版本及诊断 JSON 保留在后端，不从持久化详情直接透传。审查 FAILED 与 DOUBTFUL 的 GET 仍返回 200。

同步了数据库、后端、前端设计、文档索引、基础开发计划和项目说明；[手动 HTTP 操作](../../crates/backend/README.md#手动启动与重跑)给出从最新快照生成预期 ID、主动启动及查询的步骤。GUI 仍待 L05，不将本次 HTTP 验收计入原浏览器结果。

#### 12.4.1 实际检查

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：最终通过。首次 Clippy 报告接纳结果枚举的变体大小差异；将携带审查详情的变体改为 Box 后修正，没有增加 lint 豁免。
- `cargo test -p acorn-backend --locked --lib`：十项通过，新增必填可空预期 ID 的解析检查，覆盖遗漏、错误类型、非法 UUID、未知字段及规范化。
- 在可绑定回环端口的宿主环境执行 `cargo test -p acorn-backend --locked --test database --test workflow`：九项数据库和首轮三十八项 workflow 常规测试通过，两个真实 Isolate 验收按原规则忽略。补充题目／历史字段断言后，`cargo test -p acorn-backend --locked --test workflow published_queries_and_history_shapes` 定向通过；补充并发首次接纳后再执行 `cargo test -p acorn-backend --locked --test workflow`，最终三十九项常规测试通过、两个真实验收忽略。
- [八项新增 workflow 检查](../../crates/backend/tests/cases/review_workflow.rs)沿用受控 Axum、gRPC 和临时 SQLite。验证禁用题目的手动 CE／首例 WA、非 finished 拒绝、404、服务禁用和停止边界、队列满保留旧记录、PASS／FAILED 重跑、旧 ID 不启动下一轮、公共意见与五状态历史组合筛选。八个并发首次请求取得一个 202、七个 200、同一 review_id，模型仅收到一次 POST；已 PENDING 时另验证满队列下并发复用，不重复调用。
- 连续重跑窗口通过正式完成写入提前暴露上一轮终态、保持旧执行活动来构造；新一轮取得不同 ID，旧完成不会覆盖或误合并新交接，最终新意见可保存。意见及失败数据只由受控响应或数据库夹具构造，不代表真实模型判断。
- 取消等待检查在实际 HTTP 路由已登记操作、SQLite 写锁尚未释放时丢弃请求等待；解锁后 PENDING 交接并保存 PASS。独立后端子进程另在登记后断开 TCP 等待并发送停止信号，锁未释放期间健康 GET 仍为 200、新手动 POST 为 BackendStopping；解锁后保存 FAILED/ReviewInterrupted，进程正常退出，未发模型请求。
- 变更 Markdown 的本地链接与锚点、代码块闭合、既有编号保留、项目说明 shell／内嵌 Python 语法，以及 `git diff --check`：通过。

未调用真实模型、读取开发凭据、修改实际部署数据库或执行完整 workspace、真实 Isolate、前端或浏览器验收。下一步 L05 接入结果页审查区、手动按钮、联合轮询和历史筛选。

### 12.5 L05：结果页、轮询与历史筛选

L05 已完成。[前端类型与请求](../../web/src/api/index.ts)同步题目 review_mode、详情及摘要 review_status、七字段公共 review 和手动 POST，固定发送 expected_review_id，不复用创建提交的幂等键或恢复流程。[审查区](../../web/src/components/SubmissionReview.vue)独立显示五状态、触发方式、请求与完成时间、模型、有效意见及失败说明；疑点展示类别、从 1 开始的闭区间行号、具体证据及可能的正常解释。模型内容使用 Vue 文本插值，不解析 HTML 或 Markdown，不读取当前草稿充当历史源码。

[手动请求](../../web/src/composables/useReviewRequest.ts)只由用户点击发送，finished 的 CE、零通过或禁用自动策略均可操作；非 finished、PENDING、POST 在途或尚待确认时禁用按钮。重跑提示替换上次结果，接纳后立即查询。ReviewChanged 或响应不明时先 GET，不自动再次 POST；确认查询失败时保留最近快照、错误和启动禁用，手动查询成功后再决定是否操作。明确拒绝保留旧意见，离页或 ID 改变取消浏览器等待并忽略迟到响应，不宣称撤销后端操作。

[联合轮询](../../web/src/composables/useSubmissionPolling.ts)复用原串行一秒查询，机器非终态或审查 PENDING 时继续；机器及审查均结束才停止。手动操作后的查询作废较早在途 GET，避免旧快照覆盖新审查；隐藏时保留待确认标记，查询失败仍等待手动重试。[历史页](../../web/src/pages/SubmissionsPage.vue)增加五状态筛选及独立状态列，复用 URL、分页和详情返回，应用／清除筛选回到第一页并保留页长，不逐行查询详情或提供启动按钮。调整列宽，使 1024 CSS px 桌面下所有列仍可查看。

同步前端说明、相关设计、文档索引和当前进度。

#### 12.5.1 实际检查

- `pnpm format:check`、`pnpm lint`、`pnpm build`（含 `pnpm typecheck`）及 `pnpm test` 通过，十二个测试文件共六十四项。新增十一项覆盖手动请求身份及重复点击、结果不明和旧 ID 后先查询、明确拒绝、非 finished／PENDING 禁用、取消及迟到响应、联合轮询重启与旧 GET 作废、隐藏和查询失败后的确认门槛、模型文本转义及闭区间显示；既有历史测试增加 review_status 参数、往返及非法值断言。首轮新增测试存在 mock 参数类型／lint 和 Node SSR 加载 CSS 问题，修正测试设置后上述检查通过。
- 构建保留既有 Monaco 大块体积提示，构建成功；未改变编辑器依赖或加载方式。
- 使用临时本机静态服务提供构建产物，复用 Playwright、Chromium Headless Shell `156.0.8078.4` 和中文字体，时区为 `Asia/Hong_Kong`，视口为 `1440×900` 与 `1024×900`。HTTP 由浏览器路由提供受控响应，核对实际发出的 GET／POST 及请求正文，不连接部署后端或模型。
- 浏览器检查机器 running 到 finished/PENDING 持续更新及 PASS 停止；禁用策略的 CE 手动启动、双击只发一次 POST、立即查询至 DOUBTFUL、稳定失败说明、ReviewChanged 刷新、非 JSON 502 后确认 GET 失败暂停及手动恢复、队列满保留旧 PASS、按原 ID 重跑替换为新 FAILED、POST 在途离页后无迟到查询或跳转。机器结论保持独立，进入、刷新及查询恢复没有自动 POST。
- 历史检查审查筛选项、与既有条件组合传参、筛选及清除归零、页长保留、分页、详情直接刷新后返回原筛选，列表没有逐行详情请求。两种桌面视口无页面水平溢出，1024 CSS px 历史表格无水平溢出；已查看实际中文截图。模型标签、HTML 与 Markdown 样例保持文本，未执行脚本，应用日志未包含审查正文。最终完整流程未捕获页面运行异常或意外 API 请求；首轮临时脚本误用既有题目页标题选择器，修正后流程通过。

本阶段未修改 Rust、SQL 或 Proto，未重跑 Worker／Isolate 验收，未调用真实模型或读取开发凭据。受控浏览器响应只验证前端合同，不作为数据库持久化、后端自动门槛或完整链路联调的证据。L06 继续完成实际前后端链路的受控联调，以及一次最小真实 API 调用、保存和展示记录；不得将旧 L02 的一次真实调用记作完整链路验收。

### 12.6 L06：有限联调与真实调用

L06 已完成。本次自行启动临时后端、受控 gRPC Worker、受控 Responses 端点和前端静态代理，浏览器实际访问正式 HTTP 路由；没有拦截或伪造浏览器的提交、审查及历史响应。真实调用另以仅驻留内存的开发配置组装正式服务，经过手动接纳、独立审查队列、ReviewClient、SQLite 保存和 GUI 展示。未发现需要修改正式实现的问题，未新增运行依赖、协议、数据库结构或正式验证入口。

#### 12.6.1 联调方式与复现范围

数据库通过正式初始结构、开发用户及示例题 SQL 建立临时版本 3 库。受控阶段运行独立后端程序，配置中的模型密钥为假值；复用[既有 Worker 夹具](../../crates/backend/tests/support/mod.rs)，通过实际 gRPC 返回 AC、CE 和首例 WA，再由 Scheduler 保存机器终态及执行 ACK。源码 `ce`、`wa` 仅是夹具选择标识，不代表实际编译或执行结果。真实调用阶段也使用该 Worker 夹具，未执行 Isolate。

受控端点按标准 Responses 外层返回固定有效 PASS、固定有效 DOUBTFUL 或 HTTP 503，可暂停响应，以便核对已持久化的 PENDING 及浏览器轮询。它检查实际请求的模型、max、10240 token、json_schema、stream=false、store=false 和五字段输入白名单；不设置 strict。模型输入中的源码及题面采用提交固定版本的数据。

浏览器使用 Chromium Headless Shell `156.0.8078.4`、`1024×900` 桌面视口和 `Asia/Hong_Kong` 时区，访问本次 `pnpm build` 产物，静态代理将 `/api` 原样转发到临时后端。逐项比较数据库记录、GET 公共 DTO 和实际页面；已查看中文结果页及历史页截图。以下流程和既有操作说明可用于复核相同合同，受控后端检查可通过第 12.6.4 节的 workflow 命令重复运行。

#### 12.6.2 受控完整链路

| 路线 | 操作与实际结果 |
| --- | --- |
| 自动审查 | 临时题目 v1 使用 ENABLE-ALL，从 GUI 提交[公开 Python 样例](../../examples/submissions/a-plus-b.py)。Worker 夹具返回 AC，数据库保存 finished/PENDING，模型端暂停期间页面继续 GET 且重跑按钮禁用；释放响应后保存并显示 PASS，页面停止轮询。浏览器未发送手动审查 POST，模型端收到一次请求 |
| 禁用题目的手动 CE | 发布内容相同且模式为 DISABLED 的 v2，从 GUI 提交夹具标识 ce。初态为 finished/CE、审查 DISABLED；双击“启动审查”只发送一次 expected_review_id=null 的 POST。经过 PENDING 后保存、展示受控 DOUBTFUL，机器 CE 及编译诊断保持不变 |
| 首例 WA、失败与重跑 | v2 提交夹具标识 wa，确认首个失败用例为 1、初态为 DISABLED。手动启动后受控 HTTP 503 保存 FAILED/ReviewApiFailed、result=null，页面显示失败；用户主动重跑携带失败审查 ID，生成新 ID 和 PENDING，清空旧错误，随后保存 PASS。机器 state、verdict、终态时间及首例失败详情均保持不变 |
| 展示与刷新 | 数据库与公共 review 的 ID、触发方式、时间、模型、有效意见及错误一致，公共对象仅有约定七键。存疑结果显示第 1–1 行闭区间、证据和可能的正常解释；摘要中的 script 标签及证据中的 Markdown 按文本显示，未执行脚本。终态停止轮询，刷新保留最近一次审查且不新增 POST |
| 历史与往返 | 题号 1、finished、CE、DOUBTFUL 和 limit=1/offset=0 的组合筛选仅返回对应 CE 记录。进入详情、直接刷新后返回历史，全部筛选及分页参数保留；1024 CSS px 下页面与历史表格无水平溢出 |

受控阶段合计四次模型 POST：一次自动审查、两次手动启动及一次主动重跑；没有自动重试。固定 DOUBTFUL 只验证保存和展示合同，不作为 CE 存在作弊疑点或真实模型语义效果的证据。

#### 12.6.3 一次真实 API 调用

按[第 11.1 节](../08-llm-code-review-design.md#111-仅供开发的-endpoint-与密钥验证方式)，仅仓库外临时开发入口读取 `.env`，将 endpoint 和密钥注入内存配置；没有改写磁盘后端 TOML、把密钥放入命令行或让前端读取凭据。实际发送目标为配置的 DeepSeek Responses HTTPS 接口，输入只含公开示例源码、语言、固定公开标题和题面及机器 verdict，不含隐藏用例或用户身份。

| 项目 | 实际记录 |
| --- | --- |
| 样例 | `examples/submissions/a-plus-b.py`，保存源码逐字节一致；临时题目 v1，DISABLED，机器 AC 由 gRPC 夹具返回 |
| 请求 | `deepseek-flash`，`reasoning.effort=max`，`max_output_tokens=10240`，`text.format.type=json_schema`，`review-policy-v1` |
| 触发 | GUI 点击一次“启动审查”，POST 携带 expected_review_id=null，接纳后立即 GET 并继续轮询；没有重复 POST 或主动重跑 |
| 响应 | HTTP 200，完整有效 PASS、findings 为空，稳定错误为空；模型认为该源码读取两个整数并输出其和，没有发现约定范围内的规避证据 |
| 实际用量 | 输入 957 token、输出 96 token，其中推理 25 token；均来自提供商响应，不估算费用或缺失值 |
| 保存与展示 | PASS、意见、响应 ID、用量、思考强度和 prompt_version 保存到数据库；公共 DTO 返回有效意见及显示元数据，页面显示模型和中文摘要。完成后停止轮询，直接刷新仍读取同一 review_id；机器 AC、终态时间及空 AC 详情保持不变 |

后端日志只有一次审查请求完成事件和一次成功终态提交事件；浏览器只发送一次手动审查 POST。人工核对该 PASS 与样例的通用读取、求和逻辑一致。本次完成一份样例的一次真实审查及其保存、展示。

#### 12.6.4 实际检查与交付边界

- `cargo test -p acorn-backend --locked --test workflow review_workflow`：十五项通过，覆盖现有审查自动／手动接纳、容量、去重、条件写入、查询及生命周期合同。
- `pnpm format:check`、`pnpm build`（含 `pnpm typecheck`）：通过，构建保留既有 Monaco 大块体积提示。实际浏览器使用本次构建产物。
- `pnpm test src/composables/useReviewRequest.test.ts src/composables/useSubmissionPolling.test.ts src/components/SubmissionReview.test.ts src/utils/historyQuery.test.ts`：四个文件、十四项通过。
- 受控完整链路及一次真实调用的桌面浏览器流程均通过，未捕获页面运行异常；受控模型正文未进入应用日志。首次临时服务因沙箱禁止绑定回环端口而启动失败，后续在允许本机监听的宿主环境完成检查。
- 临时开发入口和构建产物均不进入 Git；日志及分享记录不包含真实密钥。临时服务均已正常停止，验证入口已移除。文档格式、本地链接／锚点、代码块闭合及既有编号检查通过。

本次未修改正式 Rust、前端源码、SQL、Proto 或配置合同，未重跑全量 workspace 测试、真实 Worker／Isolate 或批量模型评测，也未升级实际部署数据库。健康页面周期获取及其浏览器错误处理见[独立记录](06-frontend-development-execution.md#5122-周期获取的验证范围)，不计入 L06。

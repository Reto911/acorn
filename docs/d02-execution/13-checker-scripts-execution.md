# Python 判题脚本实施记录

对应[设计 v1](../13-checker-scripts-design.md)。任务沿用 PS01–PS06，验收沿用 CV01–CV08。

## 1. 当前状态

PS01–PS03 已依次完成，交付结构版本 5、存储与初始化适配、完整 checker 任务合同、装配与两端接纳预算，以及双环境生命周期与文件接口，实际检查见第 5、6、9 节。checker 实际裁决与新增示例题仍待 PS04–PS05，整体联调由 PS06 验收；实际执行器继续使用原固定 trim 比较。

PS01、PS02 已分别提交为 f0f3bb7 和 e095f96。PS03 已完成并单独提交；PS04 的实施分解见第 2.4 节，尚未开始。验证安排见第 7 节。

## 2. 任务状态

任务定义与依赖见[设计第 7 节](../13-checker-scripts-design.md#7-实施任务)。

- [x] PS01 数据结构与存储。
- [x] PS02 协议、任务装配与预算。
- [x] PS03 沙箱生命周期与文件对接。
- [ ] PS04 checker 执行与结果说明。
- [ ] PS05 预设、示例题与初始化说明。
- [ ] PS06 定向验收与交付。

### 2.1 PS01 数据结构与存储

按[设计第 2 节](../13-checker-scripts-design.md#2-数据模型与源码副本)实施结构版本 5。任务范围是正式初始化、存储读取和既有数据库夹具，脚本任务装配由 PS02 接入。

- [x] **PS01.1 更新完整建表 SQL 与结构版本。** 修改 [001-initial-schema.sql](../../sql/001-initial-schema.sql)，新增 checker_scripts 的 id、name、source，删除 problem_versions.comparison_mode，增加脚本引用、默认 {} 的 JSON 对象参数和三项可空正值限额覆盖。脚本外键使用 RESTRICT，允许多题版本引用同一脚本，沿用 InnoDB 所需索引。结构版本改为 5，并同步[后端启动检查](../../crates/backend/src/database/mod.rs)；保留现有用户、题目发布指针、提交、幂等和审查约束。
- [x] **PS01.2 调整题目存储类型与读取。** 依赖 PS01.1；从[题目类型](../../crates/backend/src/problems.rs)和[题目查询](../../crates/backend/src/database/problems.rs)移除 comparison_mode。同步从[现有任务装配查询](../../crates/backend/src/database/submissions.rs)删除该列及固定比较模式检查，避免读取已移除的字段。普通题目查询继续只读取公开题面及用户语言限额；checker 私有数据在 PS02 的任务装配边界读取。
- [x] **PS01.3 同步现有种子与数据库夹具。** 依赖 PS01.1；在 [002-example-problem.sql](../../sql/002-example-problem.sql)写入设计中的最小 trim 源码，让原两数求和版本引用该记录并使用 {}，保留原语言预算和 104 个用例。同步[测试初始化](../../crates/backend/tests/support/database.rs)、[版本发布辅助](../../crates/backend/tests/support/mod.rs)及[数据库测试](../../crates/backend/tests/database.rs)中的版本复制 SQL、结构版本断言和新增字段。核对[部署初始化入口](../../scripts/deploy.py)的脚本顺序与版本提示，保持正式初始化和测试使用同一套 SQL。
- [x] **PS01.4 完成结构与存储定向检查。** 依赖 PS01.1–PS01.3；按第 4.1 节扩展现有数据库用例，验证新外键、共享引用、可修改源码、参数形状及覆盖值约束，复用固定版本、初始化和审查约束检查。按第 4.3 节执行必要检查，记录实际结果、环境缺口及失败；新增具体检查前告知覆盖内容与必要性。
- [x] **PS01.5 同步说明并收口阶段交付。** 依赖 PS01.4；更新[数据库设计](../02-database-design.md)、[SQL 说明](../../sql/README.md)、[后端说明](../../crates/backend/README.md)和受影响的现行设计、[英文根说明](../../README.md)、[中文根说明](../../README_zh-CN.md)中的结构版本与初始化合同。记录实际检查后勾选任务，历史执行记录保留原始版本与结果。

执行顺序为 PS01.1 → PS01.2／PS01.3 → PS01.4 → PS01.5，类型、查询和 SQL 在同一阶段衔接。完成条件为正式空库初始化可用、后端接受版本 5、实现和夹具不再依赖 comparison_mode，新增约束以及原有题目、提交和审查合同有对应检查。

PS01 中的最小 trim 种子是维持初始化可用所需的结构适配。PS05 再交付预设 .py 编写材料、浮点脚本与商题、专用多解夹具及完整使用说明。PS01 完成仅表示结构与存储就绪，脚本执行能力仍依赖 PS02–PS04。

### 2.2 PS02 协议、任务装配与预算

依赖 PS01，按[设计第 3 节](../13-checker-scripts-design.md#3-任务装配与消息)接入完整任务。复用既有 JudgingTask、数据库 load_task 和 Scheduler 发布流程。

- [x] **PS02.1 扩展 Proto 并生成共享类型。** 在 [worker.proto](../../proto/acorn/worker/v1/worker.proto)为 SubmitJobRequest 增加字段号 6–8 的 checker_source、checker_config_json、checker_limits，新增含三个 optional uint64 的 CheckerLimits，保留现有字段号和结果结构。按[现有生成流程](../../crates/protocol/README.md#构建与生成)重新构建生成代码，并同步受影响的任务构造点；生成输出由 Cargo／Prost 管理。
- [x] **PS02.2 装配脚本副本与有效限额。** 依赖 PS01、PS02.1；在[数据库任务装配](../../crates/backend/src/database/submissions.rs)按提交固定版本读取脚本源码、版本参数和覆盖值，以及该版本可选的 Python 语言限额。每项独立采用“题目覆盖 → Python 同项限额 → 不设限”，保留 NULL 与正值的区别，拒绝不能转换为沙箱数值的有限值。即使提交语言为 C，也从 Python 行取得 checker 默认值；缺少 Python 行不妨碍装配。脚本只在 Job 级携带，沿用短连接／事务和自有数据返回方式。
- [x] **PS02.3 衔接全部装配入口与副本语义。** 依赖 PS02.2；核对新建、queued 幂等复用／重交接及启动装入均使用同一 load_task 合同。新建完成装配后将完整任务交给[创建服务](../../crates/backend/src/submissions/creation.rs)与 [Scheduler](../../crates/backend/src/scheduler/mod.rs)，发布端直接编码内存任务。dispatching、running 启动时仅恢复等待状态。内存任务和已编码消息保留装配时的源码与参数，确实需要重新装配 queued 时读取当前脚本。
- [x] **PS02.4 接入后端配置与任务预算。** 在[后端配置](../../crates/backend/src/config.rs)及[示例](../../config/backend.example.toml)增加 request.checker_source_max_bytes = 65536 和 checker_config_max_bytes = 16384。装配时校验 checker 源码非空、UTF-8 和单项预算，校验参数为 UTF-8 JSON 对象并满足文本字节预算；合计预算在现有用户源码、用例输入和参考数据上计入这两项，沿用 checked_add 与错误报告。完整 Protobuf 继续通过现有 Kafka 编码预算检查，普通 HTTP DTO 和 LLM 输入保持既有投影。
- [x] **PS02.5 接入 Worker 接纳与可选限额校验。** 依赖 PS02.1；同步 [Worker 配置](../../crates/worker/src/config.rs)、[示例](../../config/worker.example.toml)和[公共接纳入口](../../crates/worker/src/runtime.rs)的新增请求预算、UTF-8、JSON 对象和合计预算校验。checker_limits 消息必须存在，空消息表示三项均不设限；存在的有限值必须为正且可由沙箱表示。用户 RunLimits 仍必须为正并服从原 max_run 上限；checker 未设限项保持缺失。沿用 InvalidJob 接纳失败合同，Python 语法与运行错误留给执行阶段处理。
- [x] **PS02.6 完成协议与装配定向回归。** 依赖 PS02.2–PS02.5；按第 4.2 节扩展现有协议、数据库、配置、Worker 和 workflow 检查，同步所有受影响的任务夹具。覆盖新增字段往返、限额缺失语义、共享脚本与版本参数、源码副本与重新装配、三类装配入口，以及新增字节预算和公开响应边界；复用原有幂等、固定版本和 Kafka 大消息检查，不重复完整故障矩阵。
- [x] **PS02.7 同步合同并收口阶段交付。** 依赖 PS02.6；同步受影响的[Worker 设计](../01-worker-design.md)、[数据库设计](../02-database-design.md)、[后端设计](../03-backend-design.md)、[Kafka 设计](../10-kafka-judging-design.md)、协议及服务说明和两端配置示例。按第 4.3 节完成格式、编译、Clippy 与相关测试，在本记录登记实际通过、失败和未测范围；所有新写 Rust 项补齐 Rustdoc。

PS02.1 可在 PS01 数据库工作期间先推进协议定义；装配接线须在 PS01 就绪后完成。PS02.4、PS02.5 可以分别推进，随后完成调用方回归和阶段检查。完成条件为所有装配入口产生同一合同的完整任务，源码／参数副本、可选限额与两端预算经过定向验证。

PS02 只验证任务数据的装配、编解码和接纳。将可选限额落实到 Isolate 参数、两类环境及文件交接属于 PS03；checker 执行、严格结果 JSON、CheckerFailed 归因与资源展示属于 PS04。CV05、CV06、CV08 在本阶段仅记录已覆盖的装配与协议部分，完整执行及真实链路由后续任务验收。

### 2.3 PS03 沙箱生命周期与文件对接

依赖已完成的 PS02，按[设计第 5 节](../13-checker-scripts-design.md#5-沙箱与资源)将环境从阶段提升为 Job 内复用。沿用现有 Isolate 封装、停止通道和执行位置，checker 结果预算与实际裁决由 PS04 接入。

- [x] **PS03.1 配置双 box 与收尾预算。** 在 [Worker 配置](../../crates/worker/src/config.rs)和[示例](../../config/worker.example.toml)增加 sandbox.checker_box_id，检查其与用户／编译 box_id 不同。按实际收尾顺序校验 shutdown_timeout_ms 能覆盖活动阶段停止及两个已初始化环境的清理预算，保留溢出检查；同机多个 Worker 的全部 ID 由部署分配。同步受影响的配置夹具和多 Worker 示例。
- [x] **PS03.2 拆分初始化、运行与清理。** 在[沙箱类型](../../crates/worker/src/sandbox/mod.rs)和 [Isolate 实现](../../crates/worker/src/sandbox/isolate.rs)调整现行 run 绑定的生命周期，使已初始化环境可串行运行多次并在 Job 收尾时清理。复用既有命令构建、配额、收集、元数据及停止逻辑。C 编译仍独立初始化、执行、导出、清理，之后才初始化持久至 Job 结束的用户环境；checker 环境在首次需要时初始化。
- [x] **PS03.3 落实可选执行限额与绝对期限。** 依赖 PS03.2；调整[阶段限额表示](../../crates/worker/src/language/mod.rs)及调用方，使 checker CPU、墙钟和内存可逐项缺失，缺失时不添加对应 Isolate 参数，有限值保留 PS02 的正值与可表示范围。编译和用户运行继续显式提供有限值，不给 checker 套用用户 max_run 或剩余额度。所有阶段继续传递接纳时建立的同一 StageControl 绝对期限，不因用例、环境或 checker 启动而刷新。
- [x] **PS03.4 共用受控固定文件准备。** 依赖 PS03.2；将程序、输入、参考数据、脚本和配置的放置纳入同一受控常规文件复制入口。重新放置协议指定条目时不跟随残留链接，执行前清空本例输出、结果和诊断的文件及收集状态；无法准备时显式报告执行故障。保留其他用户文件及原磁盘／inode／文件限额，保留编译产物导出的常规文件和大小检查。
- [x] **PS03.5 接入 Job 环境与 stdout 文件交接。** 依赖 PS03.2–PS03.4；调整[执行器](../../crates/worker/src/executor.rs)及 JobContext，持有用户和 checker 两类环境及任务私有副本。用户环境跨例复用，每例重新启动进程并放入原始程序和当前 stdin；程序副本的修改不影响下一例。用户进程树确认停止、输出收集完成后，将有界 stdout 保存为 Worker 私有受控文件，供 checker 取得 output.bin；OLE 不以截断结果继续。checker 的 input.bin、answer.bin、output.bin、config.json 和脚本复制接口就绪，参考数据、checker 源码及私有参数始终不进入用户环境。
- [x] **PS03.6 完成双环境公共收尾与占用保留。** 依赖 PS03.5；正常、用户失败和执行故障共用 Job 收尾。确认活动进程树停止后清理已初始化环境与私有文件；已停止环境清理失败只警告。任一停止未确认时，保留仍可能使用的环境、私有资源及执行占用，Executor::is_available 同时反映两环境状态。继续衔接 [runtime](../../crates/worker/src/runtime.rs) 的 StopUnconfirmed、BLOCKED、状态 RPC、终态交接与 Kafka 退出任务组合同，停止仍通过通道请求并等待执行 future 完成。
- [x] **PS03.7 完成生命周期与隔离定向检查。** 依赖 PS03.1–PS03.6；按第 7.1 节扩展现有配置、sandbox_lifecycle、executor 和共享夹具，覆盖同 Job 复用／跨 Job 初始化、固定条目重放、其他文件保留、双环境可见范围、限额缺失和收尾边界。真实 Isolate 定向检查使用部署端分配的两个空闲 ID；受控管理器的编排结果与真实隔离结果分别记录，必要环境缺失时保留未完成范围。
- [x] **PS03.8 同步当前沙箱合同并记录交付。** 依赖 PS03.7；同步[Worker 设计](../01-worker-design.md)、[Worker 说明](../../crates/worker/README.md)、根双语说明、相关初始化／部署及验收示例，替换每例新环境和单 box 的旧描述。使用 Isolate 返回的本次 CPU／墙钟时间及用户环境 cg-mem 历史峰值，不再次扣除累计 CPU、不求和内存、不合并 checker 计量。执行第 7.3 节对应检查，登记实际结果后完成任务；HTTP／页面资源说明由 PS04 接续同步。

PS03.1 与 PS03.2 可分别推进，随后完成限额及文件接口，再衔接 Job、公共清理和验证。完成条件为用户执行已按同 Job 复用，checker 底层环境及四文件接口经定向验证，可选限额与原停止／占用合同完整衔接。实际 checker 命令、结果收集策略和裁决由 PS04 接通；本阶段仅验证对应底层能力，不据此将 CV06–CV08 的完整执行链路记为通过。

### 2.4 PS04 checker 执行与结果说明

依赖 PS02、PS03，按[设计第 4 节](../13-checker-scripts-design.md#4-脚本协议)接通统一 Python 校验，并同步[资源展示语义](../13-checker-scripts-design.md#53-复用的计量语义与释放)。

- [ ] **PS04.1 接入 checker 结果与诊断预算。** 在 Worker 配置及示例增加 checker.result_max_bytes = 4096、diagnostic_max_bytes = 65536 和必要正值校验。扩展[输出策略与收集器](../../crates/worker/src/sandbox/output.rs)：checker stdout 超限停止并形成故障，stderr 超限仅保留前缀、标记截断并继续排空，不改变合法裁决。现有 OutputPolicy::Run 会把任一流超限都视为用户 OLE，须按 checker 合同区分；每例使用新的收集状态。
- [ ] **PS04.2 严格解析唯一 JSON 裁决。** 在 Worker 内定义具有 Rustdoc 的小型结果类型和解析入口，只接受完整 UTF-8 对象、唯一 verdict 字段及 AC／WA 值，允许首尾空白。拒绝缺失、重复、未知字段、错误类型、其他值、非对象、空输出、多个 JSON 值和调试文字。保留重复字段证据，不能先解析成会覆盖同名键的通用 Value；源码与参数的语义错误仍由脚本执行报告。
- [ ] **PS04.3 接通 CHECKING 与固定命令。** 依赖 PS03、PS04.1–PS04.2；拆开 executor 中的用户运行事实裁决和固定 trim 比较。CE 或用户已确认 MLE／OLE／TLE／RE 时保持原结论并跳过 checker；正常用户运行后，使用配置的 Python 执行 -I -B -S checker.py input.bin answer.bin output.bin config.json，stdin 为空。接入 PS03 的独立环境及受控文件，沿用 CHECKING 阶段，移除内置比较器。
- [ ] **PS04.4 对齐实际退出、故障归因和私有诊断。** 依赖 PS04.3；等待 checker 实际退出、进程树确认停止和完整收集后，只有无信号、无限额触发且 Isolate 元数据中的脚本退出码为 0 才解析裁决。语法、异常、非零退出、独立限额和协议错误归为 CheckerFailed，提前输出 AC 不能覆盖后续故障。初始化／准备／启动／停止／收集错误沿用既有相应故障；JobDeadlineExceeded、WorkerShutdown 和 StopUnconfirmed 保持原优先边界。内部英文日志记录运行事实、截断标记及最多 4096 UTF-8 字节的 stderr 摘要，非法字节转换后也须守住字节上限。公开 diagnostic 仅含用例序号和稳定原因，继续受 512 UTF-8 字节限制，不拼入原始 checker 输出或脚本内容。
- [ ] **PS04.5 对齐完成数量、首败和用户资源。** 依赖 PS04.3–PS04.4；用户失败按既有裁决语义计数，checker 得到 AC／WA 后才增加本例 completed_cases，checker 自身故障保持先前数量。WA 首败短路，FailedCase 继续使用本次用户运行的输出、退出事实和资源，不误用 checker outcome。CPU／墙钟表示本次用户运行，内存表示用户环境观测到的历史峰值，可能包含此前用例；更新 Worker 与[后端资源类型](../../crates/backend/src/submissions.rs)的 Rustdoc，保留缺失与显式零的区别。
- [ ] **PS04.6 衔接失败保存与私有数据边界。** 依赖 PS04.4–PS04.5；核对[后端终态类型](../../crates/backend/src/submissions.rs)、[事件校验](../../crates/backend/src/scheduler/validation.rs)及保存链路，复用已有 failed 分支接纳 CheckerFailed，验证无 verdict、稳定诊断、先前完成数量和首个有效终态保持。既有故障码已支持非空字符串，无需新增白名单或结果结构。LLM 继续使用原始用户源码、语言、固定公开题面和机器 verdict，checker 私有数据不进入输入；机器 failed 沿用既有不进入审查的合同。
- [ ] **PS04.7 同步结果页面与现行说明。** 依赖 PS04.5–PS04.6；调整[结果组件](../../web/src/components/SubmissionOutcome.vue)、[前端资源类型](../../web/src/api/types.ts)及使用说明，明确用户环境历史内存峰值和当前用户运行的时间，执行失败继续使用既有展示。同步 Worker、后端、前端设计及相关服务、根双语说明，替换固定 trim、每阶段初始化及资源均为失败用例独占的旧描述。保留现有结果字段、单位、文本转义和私有诊断边界。
- [ ] **PS04.8 完成定向裁决回归与阶段收口。** 依赖 PS04.1–PS04.7；按第 7.2 节验证严格 JSON、退出事实、两类输出预算、CheckerFailed、完成数量、WA 用户事实和已有 HTTP／LLM 投影。复用原用户错误、资源优先级、首败、期限、停机和停止未确认覆盖，以及现有前端结果检查。按第 7.3 节执行实际改动对应命令，记录真实／受控范围和失败；全部必要项完成后勾选任务。

PS04.1、PS04.2 可分别推进，PS03 文件与环境接口就绪后接通 PS04.3，再依次完成故障、进度资源、保存和展示。完成条件为统一 checker 已参与用例裁决，成功退出与严格 JSON 共同成立才形成 AC／WA，故障和公开结果符合合同。PS05 接续预设编写材料、浮点题及多解夹具；PS06 汇总真实 Kafka → Worker／Isolate → MySQL 和必要页面验收。此处的受控检查不替代真实隔离或整体交付。

## 3. 设计定稿记录

已落实统一脚本表与可修改源码、四文件输入、stdout JSON 与成功退出码 0、同提交沙箱复用、Python 默认限额与缺省不设限、Job 总期限覆盖 checker，以及默认绝对／相对容差 1e-6。

复审后精简方案排除说明和重复的仓库检查规则。checker 内部失败日志的 stderr 摘要上限为 4 KiB，全文不持久化；JobFailure.diagnostic 仍遵守 512 UTF-8 字节上限。

新增正式设计和本记录，移除临时讨论稿并更新索引及相关现行设计的衔接说明。未修改 Rust、Proto、SQL 或前端实现，未运行构建、数据库、沙箱、浏览器或模型验收。

实际文档检查：8 份文档通过 Prettier，174 处本地链接与锚点有效，现有文档章节编号保持不变。新设计主章节连续，PS01–PS06 与执行清单对应，任务依赖从表格解析后检查无环，CV01–CV08 唯一且连续。旧讨论稿及其入链已移除，git diff --check 通过。

## 4. PS01–PS02 定向验证安排

以下是实施时的覆盖计划，实际结果见第 5 节起的阶段记录。优先扩展既有用例，每项新增或扩展检查前按仓库要求说明具体覆盖及必要性。

### 4.1 PS01 覆盖与复用入口

| 入口                                                                                            | 覆盖内容与必要性                                                                                                                                 |
| ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| 现有 database 初始化与 startup_checks_and_finite_lock_wait                                      | 使用正式 SQL 在独立空库初始化并接受结构版本 5，保留错误版本拒绝、空用户表及有限锁等待检查，证明新结构没有破坏启动合同。                          |
| 现有 invalid_task_data_and_foreign_keys                                                         | 补充脚本共享引用、无效引用／删除被引用脚本的拒绝、源码可 UPDATE、参数只接受 JSON 对象、三项覆盖接受 NULL／正值并拒绝零／负值，验证新增存储约束。 |
| 现有 published_queries_and_fixed_version_creation 与 review_policy_interruption_and_constraints | 适配版本复制与发布夹具，保留两数求和 104 例、固定版本、本人提交及审查约束，确认字段替换不改变既有业务合同。                                      |

### 4.2 PS02 覆盖与复用入口

| 入口                                                                  | 覆盖内容与必要性                                                                                                                                                                                            |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [protocol 测试](../../crates/protocol/tests/)                         | 扩展既有 wire contract 和 Kafka record 检查，验证新增源码／参数原字节、可选限额的缺失／显式零／正值以及消息不存在／空消息的区别；编码层保留数值，合法性由接纳边界判断。                                     |
| [backend 数据库测试](../../crates/backend/tests/database.rs)          | 在既有固定版本和 queued 装配场景验证覆盖优先级、Python 默认及缺少默认来源时为 None；同一脚本由多个版本共享但参数独立，旧任务／编码消息不随 UPDATE 改变，重新装配取得新源码。覆盖 CV05 和 CV06 的装配部分。  |
| [backend workflow](../../crates/backend/tests/workflow.rs) 及配置检查 | 复用新建、幂等 queued 重交接及混合状态启动场景，核对完整 payload 和 dispatching／running 不重装配；验证新增预算默认值、边界拒绝，以及公开题目响应和 LLM 投影不含私有 checker 数据。只在现有断言不足处补查。 |
| Worker 公共接纳及配置测试                                             | 将合法脚本任务纳入现有夹具，定向验证源码／参数非法、缺少限额消息、有限值零／不可表示、单项／合计预算超限与空限额消息合法；保留用户 RunLimits 正值与 max_run 检查，证明 checker 增量没有放宽用户预算。       |
| 两端既有 Kafka 编码预算检查                                           | 将 checker 源码和参数计入原始任务合计与完整 Protobuf 长度，区分原始预算与编码后预算。PS02 验证编解码及调用方行为；Kafka → Worker／Isolate → MySQL 的真实闭环在 PS06 完成。                                  |

### 4.3 实施命令与阶段边界

Rust 实施时沿用项目已有命令，按改动选择相关测试；格式、编译和规定的 workspace Clippy 均需执行：

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p acorn-protocol --locked
cargo test -p acorn-backend --locked --lib --test config_results
cargo test -p acorn-backend --locked --test database --test workflow -- --test-threads=1
cargo test -p acorn-worker --locked
```

PS01 主要执行后端相关检查；PS02 增加协议及 Worker 相关检查。数据库和 workflow 沿用[既有测试配置](../../config/backend-test.example.toml)及[独立库流程](../../crates/backend/README.md#检查命令)，从环境取得 ACORN_TEST_DATABASE_CONFIG 和 MYSQL_PASSWORD。实际筛选命令、结果和未完成项在实施时登记。

规划阶段仅检查变更文档格式、稳定编号、本地链接／锚点和差异空白。Rust、数据库、Kafka、Isolate、浏览器及模型检查当时均未执行，PS01–PS06 保持未完成。

该阶段补充 PS01 的 5 项和 PS02 的 7 项实施分解、依赖、完成条件及定向验证安排，明确最小 trim 种子的阶段归属和 PS03–PS06 的后续边界。文档格式化后通过 Prettier；38 个本地链接／锚点、原有标题与章节编号、6 项主任务及 12 项分解任务的未完成状态、代码块闭合和 git diff --check 检查通过。当时未修改程序、协议、SQL 或配置，未提交 Git。

## 5. PS01 实施与检查

完整初始化结构已更新为版本 5，新增 checker_scripts、题目版本引用、默认 {} 的对象参数和三项可空正值覆盖。后端启动、公开题目存储类型及查询移除 comparison_mode；现有两数求和种子引用内嵌 trim 源码，语言预算与 104 个用例保持原样，正式初始化仍使用原三个 SQL 脚本。

版本发布与复制夹具同步新增字段，现有数据库用例补充共享引用、外键 RESTRICT、源码更新、参数形状和限额约束。测试还核对 SQL 装入后的 Python 空白转义字面量，确认源码按原字节存储。数据库、后端、Kafka、前端及审查设计和初始化使用说明同步当前结构版本；历史执行记录保留原始版本。

| 实际检查                                                                                                             | 结果与范围                                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| cargo fmt --all -- --check                                                                                           | 通过。                                                                                                                              |
| cargo check --workspace --all-targets --locked                                                                       | 通过，另有受影响后端测试目标的定向编译通过。                                                                                        |
| cargo clippy --workspace --all-targets --locked -- -D warnings                                                       | 通过。                                                                                                                              |
| cargo test -p acorn-backend --locked --lib --test config_results --test database --test workflow -- --test-threads=1 | 22 项库测试、10 项配置测试、14 项真实 MySQL 数据库测试和 32 项常规 workflow 通过；12 项需 Kafka／Isolate 等环境的集成用例保持忽略。 |

数据库及 workflow 使用 MySQL 8.4.11，每例以正式 SQL 初始化独立测试库并在完成后删除。开发临时入口按既有约定向测试进程供给连接配置及密码，正式程序和测试未增加 .env 加载。格式、编译、Clippy 和实际检查通过后完成 PS01；未运行 checker、真实 Kafka／Isolate、浏览器或模型验收，PS02–PS06 当时仍未交付。

## 6. PS02 实施与检查

SubmitJobRequest 新增字段号 6–8 的 checker_source、checker_config_json 和 checker_limits，CheckerLimits 三项使用 optional uint64；原字段号及结果结构保持不变。按既有 Cargo／Prost 构建生成代码。Worker 引用 workspace 已有 serde_json，Cargo.lock 通过 Cargo 刷新，仅增加该 crate 的依赖关系。

后端 load_task 联查固定题目版本、脚本源码与参数，以及逐项 COALESCE 的覆盖值／同版本 Python 限额。缺少 Python 行时保留对应 None；即使提交语言为 C，也使用 Python 默认。有限 checker 值要求 1..=2147483647，不套用用户 max_run 门限。新建、queued 幂等复用／重交接和启动装入继续共用该入口，Scheduler 使用完整内存任务，dispatching／running 启动只恢复等待身份。

后端和 Worker 均增加默认 65536／16384 字节的 checker 源码／参数预算，验证源码非空 UTF-8、参数 JSON 对象和新增数据参与原始载荷合计。Worker 拒绝缺少 checker_limits 消息，接受空消息，并独立验证三项有限值；用户 RunLimits 正值和 max_run 约束保持不变。完整 Protobuf 编码仍由既有 Kafka 记录预算检查。

定向检查扩展既有协议、配置、固定版本、启动恢复与 Worker 接纳用例，并新增一个合并数据库场景验证限额、非法输入和精确字节边界。已验证数据库 UPDATE 不改变装配任务或已编码消息，queued 重新装配读取当前源码；共享脚本的不同题目版本分别取得自己的参数。公开题目响应增加私有字段不泄漏断言，LLM 沿用既有明确输入投影及审查 workflow。所有合法任务夹具同步新字段；用于真实 Kafka 消息往返的夹具也已适配，但本轮未运行其真实 broker 检查。

| 实际检查                                                                                                             | 结果与范围                                                                                                |
| -------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| cargo build -p acorn-protocol --locked                                                                               | 通过，生成新协议类型。                                                                                    |
| cargo fmt --all -- --check                                                                                           | 协议测试导入经统一格式化后通过。                                                                          |
| cargo check --workspace --all-targets --locked                                                                       | 通过。                                                                                                    |
| cargo clippy --workspace --all-targets --locked -- -D warnings                                                       | 通过。                                                                                                    |
| cargo test -p acorn-protocol --locked                                                                                | 6 项通过，覆盖新增字段、存在性和编码预算。                                                                |
| cargo test -p acorn-worker --locked                                                                                  | 22 项常规检查通过；7 项需要真实 Kafka／Isolate 的检查保持忽略，受控执行器检查不作为隔离证据。             |
| cargo test -p acorn-backend --locked --lib --test config_results --test database --test workflow -- --test-threads=1 | 22 项库测试、10 项配置测试、15 项真实 MySQL 数据库测试及 32 项常规 workflow 通过；12 项集成用例保持忽略。 |

数据库验证继续使用独立测试库和正式初始化 SQL，MySQL 为 8.4.11。测试仅在其自有库临时解除参数 CHECK 来验证后端自身的 JSON 边界，完成后恢复约束；所有正常及拒绝创建的数量核对通过，测试库正常收尾。

PS01、PS02 的任务及分解清单已完成。CV05 的共享引用、版本参数和装配副本要求，以及 CV06 的限额来源／缺失／可表示范围、CV08 的原始／编码预算与既有 HTTP／审查合同已有对应检查。实际 checker 运行、独立限额生效、Job 总期限、双环境复用及真实 Kafka → Worker／Isolate → MySQL 流程仍待 PS03–PS06，本轮未运行真实 Kafka／Isolate、浏览器、外部模型、静态链接构建或部署验收。

17 份变更 Markdown 通过 Prettier，377 个本地链接有效，其中 158 个包含锚点；333 个原有标题按原顺序保留，新增章节、任务编号和代码围栏检查通过。主任务为 2 项完成／4 项未完成，12 项 PS01／PS02 分解均完成；中英文根说明互链及 15 组命令／配置代码块一致，仅提示和占位说明按语言翻译。git diff --check 通过。

PS01 的结构与存储改动已单独提交为 f0f3bb7；PS02 的协议、完整任务装配与接纳预算随本节单独提交。拆分后的 PS01 暂存内容另经 cargo fmt --all -- --check 和 cargo check --workspace --all-targets --locked 检查通过，完整工作树保留已验证的 PS02 实现。

拆分检查曾复用同一构建目录，随后首次检查 PS02 时读取旧协议生成缓存，出现缺少 checker 字段的编译错误。通过 cargo clean -p acorn-protocol 清理该包构建缓存并按现有流程重新生成后，cargo build -p acorn-protocol --locked、cargo check --workspace --all-targets --locked 及 6 项协议测试均通过；源码未作修改，首次失败保留在本记录。

## 7. PS03–PS04 定向验证安排

以下为 PS03–PS04 的验证安排，PS03 实际结果见第 9 节。每项新增或扩展检查前，先告知用户具体覆盖与必要性；只补充本次改变的合同。

### 7.1 PS03 生命周期与文件边界

| 复用入口                                                                                                                     | 覆盖内容与必要性                                                                                                                                                                                          |
| ---------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [config_language](../../crates/worker/tests/config_language.rs)                                                              | 验证两个 box ID 不同、双环境收尾预算以及编译／用户有限值仍有效，避免配置无法兑现新的环境与停止合同。                                                                                                      |
| [sandbox_lifecycle](../../crates/worker/tests/sandbox_lifecycle.rs) 和[共享管理器](../../crates/worker/tests/support/mod.rs) | 管理器按 box ID 分配独立目录，记录初始化／运行／清理次数；验证同环境多次执行、可选限额缺省参数、固定条目残留链接不被跟随。复用清理告警、停止确认及未确认保留检查，验证拆分生命周期仍保持安全释放边界。    |
| [executor](../../crates/worker/tests/executor.rs)                                                                            | 调整旧阶段计数断言，验证每例恢复原始程序、普通文件同 Job 保留／跨 Job 不保留、stdout 不串例，以及异常路径共用清理。用户运行接入生产流程；checker 底层接口在定向沙箱场景验证，完整 checker 裁决留给 PS04。 |
| [isolate_real](../../crates/worker/tests/isolate_real.rs) 的现有真实阶段入口                                                 | 以专用双 ID 验证两环境文件可见范围、同 Job 复用／跨 Job 重建和实际限额参数。仅将真实执行过的项记作隔离结果；受控管理器只证明编排。                                                                        |

### 7.2 PS04 裁决、故障与展示

| 复用入口                                                                    | 覆盖内容与必要性                                                                                                                                                                                         |
| --------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Worker checker 解析入口及现有 executor／输出收集检查                        | 用少量分组用例覆盖明确的 JSON 接受／拒绝合同；扩展已有执行场景，验证退出 0 与合法 JSON 共同成立、提前 AC 后非零退出不成功、stdout 超限不接受前缀、stderr 截断仍可裁决，区分 checker 故障和用户 verdict。 |
| 现有 executor 进度、资源与停止检查                                          | 覆盖首例及先前已有 AC 后的 checker 故障、WA 保留用户运行事实、两环境资源不混用及内存历史峰值；复用 MLE／OLE／TLE／RE 优先级和 Deadline／Shutdown／StopUnconfirmed。                                      |
| backend 既有终态、数据库和 workflow                                         | 定向保存 CheckerFailed，验证 failed／无 verdict／有界稳定诊断和重复终态不覆盖；验证响应没有 checker 原始 stdout／stderr。优先扩展现有场景，避免为同一失败合同另建测试体系。                              |
| [LLM 输入检查](../../crates/backend/tests/llm_review.rs)与既有审查 workflow | 复用 request_contract_and_valid_opinions 的五字段断言及 failed 提交审查拒绝，必要时补查本次接线；不重复外部模型效果评估。                                                                                |
| [结果组件测试](../../web/src/components/SubmissionOutcome.test.ts)          | 复用执行失败、用户输出、可选资源及显式零的渲染回归，配合构建和必要桌面页面定向查看核对新的计量说明；不为静态文案新增镜像测试。                                                                           |

### 7.3 命令、环境与验收边界

两阶段 Rust 实施沿用现有检查：

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p acorn-worker --locked
```

PS03 的真实 Isolate 检查复用 [Worker 说明中的阶段和执行器命令](../../crates/worker/README.md#isolate-阶段检查)。实施时先将夹具扩展为接受每个 Worker 的两个专用空闲 box ID，并同步命令说明；双 Worker 夹具须分配四个互不冲突的 ID。记录实际调用和覆盖范围，不将现有单 ID 检查直接写成双环境通过。

PS04 按改动运行既有后端 lib／config_results、相关 database／workflow；数据库环境沿用第 4.3 节的独立测试库合同。LLM 客户端投影需要复验时使用既有 cargo test -p acorn-backend --locked --test llm_review，受控端点只证明输入与客户端行为。前端改动在 web 中使用既有 pnpm format:check、lint、typecheck、build 和相关 test；源码与页面功能未变的范围不重复验收。

CV06 的实际 checker 独立限额与 Job 总期限、CV07 的完整双环境执行及 CV08 的真实消息／数据库闭环须按真实覆盖登记。PS03 验证底层生命周期与文件能力，PS04 补齐实际裁决及结果合同，PS06 汇总真实链路，不能以协议、受控管理器或页面静态检查替代后续验收。

## 8. PS03–PS04 规划记录

PS01、PS02 分别提交后，补充 PS03、PS04 各 8 项实施分解、依赖、完成条件和定向验证安排。明确双环境及文件接口由 PS03 交付，checker 结果／诊断预算、输出策略、实际裁决、失败保存和展示由 PS04 接入，预设与完整联调分别留给 PS05、PS06。

规划文档通过 Prettier，64 个本地链接／锚点有效，原有标题与编号保持稳定，6 项主任务及 28 项分解的状态、编号和引用一致，代码围栏闭合，git diff --check 通过。PS01／PS02 的 12 项分解已完成，PS03／PS04 的 16 项分解均未开始。本次规划未新增或运行功能测试，规划文档保留为未提交修改。

## 9. PS03 实施记录

### 9.1 生命周期、文件与调用方

增加必填 sandbox.checker_box_id，检查用户／编译 ID 与 checker ID 不同，收尾预算按一次活动阶段停止加两次串行环境清理校验，并检查算术溢出。同步示例配置、根双语说明、Worker 说明及后端真实验收夹具；双 Worker 夹具要求四个 ID 互不冲突。

IsolateSandbox 拆分可复用 run_in_environment 与显式 cleanup，独立阶段 run 复用二者。C 编译仍完成初始化、编译、产物导出与清理，再初始化用户环境；用户环境同 Job 跨例复用，checker 环境首次需要时初始化。两者使用同一接纳绝对期限，CPU、墙钟和内存限额逐项可选；缺失项不传对应 Isolate 参数，编译与用户限额继续为正值。

每例以新常规文件恢复原始程序或 checker 固定输入，移除残留目录项而不跟随链接，保留其他普通文件。每次运行清空元数据并使用新的有界输出收集状态。正常用户进程树停止且收集完成后，将 stdout 保存为 Worker 私有文件；checker_files 由此准备脚本及四个输入文件，包含原始输入、参考数据、输出和配置。OLE 不用截断结果继续检查；checker 私有数据不进入用户环境。

Executor 持有两类环境，正常、用户失败与执行故障共用收尾。已停止环境清理失败仍只警告；停止未确认时保留环境、私有资源与执行占用，is_available 同时检查两环境。Job 总期限包括环境及私有文件清理，清理越过期限时不再返回成功。CPU／墙钟直接取本次元数据，内存保留用户环境历史峰值；HTTP 与页面说明由 PS04 接续。

### 9.2 实际检查

- cargo fmt --all -- --check、cargo check --workspace --all-targets --locked 与 cargo clippy --workspace --all-targets --locked -- -D warnings 通过。首次 Clippy 指出公共收尾中的两处可折叠 if，改用 let 链后通过。
- cargo test -p acorn-worker --locked：26 项常规测试通过，8 项依赖外部环境的测试保持 ignored。配置覆盖不同 ID、双环境预算不足和溢出；生命周期与执行器覆盖同 Job 复用、跨 Job 重建、固定程序和链接恢复、普通文件保留、空输出不串例、可选参数缺失、停止未确认禁止清理及清理跨越总期限。原 C／Python 裁决、首败、限额优先级和输出超限检查继续通过。
- 受控管理器按 box ID 提供独立目录。共享文件探针检查两个环境、脚本与四文件重放、含 NUL／非 UTF-8 字节的输入输出及私有数据可见范围；受控结果只证明编排与文件交接。
- 真实 Isolate 定向执行 real_environment_reuse_and_checker_files 和 real_executor_acceptance，各 1 项通过。前者在两个独立 box 中运行相同文件探针，覆盖同 Job 复用、跨 Job 清理／重建及三项限额全部缺失；后者覆盖独立 C 编译及 C／Python 用户运行。两个测试使用同一对部署分配的空闲专用 ID，串行运行，测试拥有的环境完成清理。

本次真实定向命令如下，使用预先确认空闲的专用 ID 220／221：

```sh
ACORN_ISOLATE_BOX_ID=220 ACORN_CHECKER_BOX_ID=221 \
  cargo test -p acorn-worker --locked --test isolate_real real_environment_reuse_and_checker_files -- --ignored --nocapture
ACORN_ISOLATE_BOX_ID=220 ACORN_CHECKER_BOX_ID=221 \
  cargo test -p acorn-worker --locked --test executor real_executor_acceptance -- --ignored --nocapture
```

6 份变更 Markdown 通过 Prettier；182 个本地链接有效，其中 64 个含锚点，95 个原有标题顺序保持。代码围栏、6 项主任务和 28 项分解的编号与状态有效，根双语说明的 15 组命令／配置代码块一致（提示、注释与占位说明按语言翻译），互链有效；git diff --check 通过。

### 9.3 交付边界

PS03 及 8 项分解已完成；PS04–PS06 保持未完成。本阶段真实检查证明双环境底层生命周期与文件边界，生产执行器仍使用固定 trim；实际 checker 命令、严格 JSON、独立超限归因、CheckerFailed 保存与资源展示由 PS04 接入。未将 CV06–CV08 完整执行链路记为通过。

本轮未运行真实 Kafka／MySQL 闭环、旧 real_stage_acceptance 全部配额场景、浏览器或外部模型检查；受影响的后端真实验收夹具已通过工作区全目标编译。整体链路与剩余验收仍由后续阶段完成。本阶段按用户要求单独提交。

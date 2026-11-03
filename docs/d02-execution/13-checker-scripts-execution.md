# Python 判题脚本实施记录

对应[设计 v1](../13-checker-scripts-design.md)。任务使用 PS01–PS09；原机制验收沿用 CV01–CV08，通用预设追加任务及完成条件见设计第 9 节。

## 1. 当前状态

PS01–PS06 已依次完成，交付结构版本 5、完整 checker 装配与接纳预算、双环境与文件接口、统一 Python checker、预设材料、浮点商题及多解夹具。真实 HTTP → Kafka → Worker／Isolate → MySQL 已完成正式题、多解与 checker 故障验收；历史／源码、LLM 输入及必要页面检查通过。实际检查见第 5、6、9、10、13、16 节，CV01–CV08 汇总见第 16.3 节。

PS01–PS05 分别提交为 f0f3bb7、e095f96、fbce955、d582852 和 cb67620。PS06 的规划、真实验收与检查说明作为同一组改动交付；任务分解见第 2.6 节，验收安排及实际结果见第 14、16 节。

PS06、PS07、PS08 分别提交为 50058d2、f8de89a、0a4fa60。PS07 的四种通用预设材料和执行器定向检查见第 19 节；PS08 的正式例题、提交样例、夹具适配及使用说明见第 20 节。PS09 及其 5 项分解已完成，八份新样例、四种错误答案与 ID 7 专用场景通过完整真实链路验收，结果见第 22 节，规划记录见第 21 节。追加任务定义见[设计第 9 节](../13-checker-scripts-design.md#9-通用预设与例题扩展任务)。

PS01–PS09 及 57 项分解均已完成；通用预设、正式种子和真实验收定义见设计第 9.3–9.5 节。原 CV01–CV08 的历史结论保持，本轮新增覆盖见第 22 节。

## 2. 任务状态

任务定义与依赖见[设计第 7 节](../13-checker-scripts-design.md#7-实施任务)及[第 9 节](../13-checker-scripts-design.md#9-通用预设与例题扩展任务)。

- [x] PS01 数据结构与存储。
- [x] PS02 协议、任务装配与预算。
- [x] PS03 沙箱生命周期与文件对接。
- [x] PS04 checker 执行与结果说明。
- [x] PS05 预设、示例题与初始化说明。
- [x] PS06 定向验收与交付。
- [x] PS07 四种通用预设脚本与定向逻辑检查。
- [x] PS08 四道正式例题、提交样例与初始化夹具适配。
- [x] PS09 真实链路验收与使用说明同步。

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

- [x] **PS04.1 接入 checker 结果与诊断预算。** 在 Worker 配置及示例增加 checker.result_max_bytes = 4096、diagnostic_max_bytes = 65536 和必要正值校验。扩展[输出策略与收集器](../../crates/worker/src/sandbox/output.rs)：checker stdout 超限停止并形成故障，stderr 超限仅保留前缀、标记截断并继续排空，不改变合法裁决。现有 OutputPolicy::Run 会把任一流超限都视为用户 OLE，须按 checker 合同区分；每例使用新的收集状态。
- [x] **PS04.2 严格解析唯一 JSON 裁决。** 在 Worker 内定义具有 Rustdoc 的小型结果类型和解析入口，只接受完整 UTF-8 对象、唯一 verdict 字段及 AC／WA 值，允许首尾空白。拒绝缺失、重复、未知字段、错误类型、其他值、非对象、空输出、多个 JSON 值和调试文字。保留重复字段证据，不能先解析成会覆盖同名键的通用 Value；源码与参数的语义错误仍由脚本执行报告。
- [x] **PS04.3 接通 CHECKING 与固定命令。** 依赖 PS03、PS04.1–PS04.2；拆开 executor 中的用户运行事实裁决和固定 trim 比较。CE 或用户已确认 MLE／OLE／TLE／RE 时保持原结论并跳过 checker；正常用户运行后，使用配置的 Python 执行 -I -B -S checker.py input.bin answer.bin output.bin config.json，stdin 为空。接入 PS03 的独立环境及受控文件，沿用 CHECKING 阶段，移除内置比较器。
- [x] **PS04.4 对齐实际退出、故障归因和私有诊断。** 依赖 PS04.3；等待 checker 实际退出、进程树确认停止和完整收集后，只有无信号、无限额触发且 Isolate 元数据中的脚本退出码为 0 才解析裁决。语法、异常、非零退出、独立限额和协议错误归为 CheckerFailed，提前输出 AC 不能覆盖后续故障。初始化／准备／启动／停止／收集错误沿用既有相应故障；JobDeadlineExceeded、WorkerShutdown 和 StopUnconfirmed 保持原优先边界。内部英文日志记录运行事实、截断标记及最多 4096 UTF-8 字节的 stderr 摘要，非法字节转换后也须守住字节上限。公开 diagnostic 仅含用例序号和稳定原因，继续受 512 UTF-8 字节限制，不拼入原始 checker 输出或脚本内容。
- [x] **PS04.5 对齐完成数量、首败和用户资源。** 依赖 PS04.3–PS04.4；用户失败按既有裁决语义计数，checker 得到 AC／WA 后才增加本例 completed_cases，checker 自身故障保持先前数量。WA 首败短路，FailedCase 继续使用本次用户运行的输出、退出事实和资源，不误用 checker outcome。CPU／墙钟表示本次用户运行，内存表示用户环境观测到的历史峰值，可能包含此前用例；更新 Worker 与[后端资源类型](../../crates/backend/src/submissions.rs)的 Rustdoc，保留缺失与显式零的区别。
- [x] **PS04.6 衔接失败保存与私有数据边界。** 依赖 PS04.4–PS04.5；核对[后端终态类型](../../crates/backend/src/submissions.rs)、[事件校验](../../crates/backend/src/scheduler/validation.rs)及保存链路，复用已有 failed 分支接纳 CheckerFailed，验证无 verdict、稳定诊断、先前完成数量和首个有效终态保持。既有故障码已支持非空字符串，无需新增白名单或结果结构。LLM 继续使用原始用户源码、语言、固定公开题面和机器 verdict，checker 私有数据不进入输入；机器 failed 沿用既有不进入审查的合同。
- [x] **PS04.7 同步结果页面与现行说明。** 依赖 PS04.5–PS04.6；按用户确认保留[结果组件](../../web/src/components/SubmissionOutcome.vue)的现有文案，在[前端资源类型](../../web/src/api/types.ts)及使用说明中明确用户环境历史内存峰值和当前用户运行的时间，执行失败继续使用既有展示。同步 Worker、后端、前端设计及相关服务、根双语说明，替换固定 trim、每阶段初始化及资源均为失败用例独占的旧描述。保留现有结果字段、单位、文本转义和私有诊断边界。
- [x] **PS04.8 完成定向裁决回归与阶段收口。** 依赖 PS04.1–PS04.7；按第 7.2 节验证严格 JSON、退出事实、两类输出预算、CheckerFailed、完成数量、WA 用户事实和已有 HTTP／LLM 投影。复用原用户错误、资源优先级、首败、期限、停机和停止未确认覆盖，以及现有前端结果检查。按第 7.3 节执行实际改动对应命令，记录真实／受控范围和失败；全部必要项完成后勾选任务。

PS04.1、PS04.2 可分别推进，PS03 文件与环境接口就绪后接通 PS04.3，再依次完成故障、进度资源、保存和展示。完成条件为统一 checker 已参与用例裁决，成功退出与严格 JSON 共同成立才形成 AC／WA，故障和公开结果符合合同。PS05 接续预设编写材料、浮点题及多解夹具；PS06 汇总真实 Kafka → Worker／Isolate → MySQL 和必要页面验收。此处的受控检查不替代真实隔离或整体交付。

### 2.5 PS05 预设、示例题与初始化说明

依赖已完成的 PS01、PS04，按[设计第 6 节](../13-checker-scripts-design.md#6-预设脚本与示例题)交付编写材料、正式商题及专用多解夹具。复用已有脚本装配与执行路径，保持结构版本 5；完整跨服务验收仍由 PS06 汇总。

- [x] **PS05.1 提供独立脚本编写材料。** 在 examples/checkers/ 新增 trim.py、float.py。两个脚本均可使用固定四文件参数及 -I -B -S 独立运行，仅使用标准库。trim 保留末尾六种 ASCII 空白语义。float 使用 bytes.split()、float 与 math.isclose，要求项数相同且值有限；先校验配置与参考数据，再判断用户输出。用户格式或非有限值形成正常 WA，参考数据和配置错误以非零退出形成 CheckerFailed。仅支持 abs_tol／rel_tol，缺省均为 1e-6，拒绝布尔值、非数值、非有限值、负值、rel_tol >= 1 及未知参数；遵守唯一 JSON 与成功退出码 0 合同。
- [x] **PS05.2 扩充正式初始化种子。** 依赖 PS05.1；扩充现有 [002-example-problem.sql](../../sql/002-example-problem.sql)，保留 trim 脚本 ID 1、两数求和题号 1／版本 1 和原 104 例。新增浮点脚本 ID 2 及题号 2／版本 1“计算两个整数的商”，参数显式为 abs_tol = rel_tol = 1e-6。公开题面说明整数范围、单个商输出、有限值与容差公式。C／Python 限额分别沿用两数求和的 1000／3000 ms／262144 KiB 和 2000／5000 ms／262144 KiB，checker 覆盖值留空以采用既有 Python 默认来源。使用 8 个固定有序用例，覆盖零、正负循环小数、小量级、分子与分母边界及大量级；参考文件保存单个有限数值。SQL 中实际存储的两份脚本与编写材料一致，保留原 DML 事务和发布顺序。
- [x] **PS05.3 提供示例提交与专用多解夹具。** 依赖 PS05.1；在 examples/submissions/ 新增 quotient.c、quotient.py，展示不同合法浮点格式；在 examples/checkers/ 新增 sum-pair.py，作为开发夹具而不写入正式种子。专用脚本从 input.bin 读取 n >= 3，从 output.bin 检查两个正整数及其和，answer.bin 为空，配置使用 {}。以同一输入的两份不同合法输出和一份非法输出覆盖 AC／AC／WA；沿用现有任务和文件接口。
- [x] **PS05.4 同步初始化调用方与现有夹具。** 依赖 PS05.2；保持 [deploy.py](../../scripts/deploy.py) 及[数据库初始化夹具](../../crates/backend/tests/support/database.rs)使用原三条 SQL 执行顺序，更新初始化完成提示。核对 [database](../../crates/backend/tests/database.rs)、[workflow](../../crates/backend/tests/workflow.rs)及共享夹具中“只有一题”、题号 2 不存在或用作临时题的假设；临时题改用与正式种子不同的 ID，查询检查按题号定位，同时保留分页、固定版本和外键校验原意。[Worker 默认任务夹具](../../crates/worker/tests/support/mod.rs)直接嵌入 trim.py 编写材料，减少测试中的重复脚本副本。
- [x] **PS05.5 完成脚本、种子与真实执行定向检查。** 依赖 PS05.1–PS05.4；按第 11 节扩展现有 executor、database 和必要 workflow 检查。验证从真实 MySQL 读回的脚本与 .py 字节一致、商题参数／语言预算／8 例完整且可装配；覆盖 trim、浮点容差与错误归因、多解及有界私有诊断。复用真实 Isolate 执行器入口，以专用双 ID 串行验证实际预设、C／Python 商题样例和多解脚本；受控管理器与真实结果分别登记。新增或扩展检查前先告知具体覆盖及必要性。
- [x] **PS05.6 完成维护、初始化与使用说明。** 依赖 PS05.1–PS05.4；新增 examples/checkers/README.md，说明四文件协议、脚本返回、预设参数、私有数据及直接数据库维护方式，明确 .py 是编写材料，装配时读取数据库源码，修改本地文件不会自动改变已存脚本。同步 [SQL 说明](../../sql/README.md)、[数据库设计](../02-database-design.md)、[Worker 说明](../../crates/worker/README.md)、[后端说明](../../crates/backend/README.md)、根双语 Quick Start 与[文档索引](../README.md)的两个正式示例题、初始化结果和样例入口。说明同脚本可共享、参数属于版本、已装配副本不随脚本修改变化，完整任务重新装配时读取当前源码；结果页按用户确认保留现有文案。
- [x] **PS05.7 验证并收口阶段交付。** 依赖 PS05.5–PS05.6；按第 11.3 节完成实际改动所需检查，记录通过、失败、受控／真实范围与剩余工作，再勾选任务。交付两种预设、两道正式种子题和专用开发夹具，保持数据库作为运行时脚本来源；为 PS06 留下可直接使用的样例及现有联调入口。

执行顺序为 PS05.1 → PS05.2／PS05.3 → PS05.4 → PS05.5／PS05.6 → PS05.7。完成条件为正式空库初始化包含两题且原两数求和不退化，数据库脚本和编写材料一致，浮点与多解通过统一 checker，用户 WA 与数据／配置故障归因明确。PS05 不以 Worker 或受控终态检查替代 PS06 的完整 Kafka → Worker／真实 checker → MySQL 闭环。

### 2.6 PS06 定向验收与交付

依赖已完成并分别提交的 PS01–PS05，按[设计第 8 节](../13-checker-scripts-design.md#8-验收要求)收口 CV01–CV08。复用已有数据库、协议、Worker、真实 workflow 和前端入口；只补充完整链路中缺少的覆盖，实际结果继续区分受控检查、真实 Isolate 和跨服务业务链路。

- [x] **PS06.1 核对验收覆盖与联调资源。** 按第 14.1 节汇总第 5、6、9、10、13 节的实际结果，确认已有覆盖与剩余缺口。沿用[独立 MySQL 库](../../crates/backend/tests/support/database.rs)及[专属 Kafka topic／group](../../crates/backend/tests/support/broker.rs)，从现有环境供给测试配置、MYSQL_PASSWORD 和 Kafka 地址；分配两个空闲且专用的 Isolate ID，串行使用。保留正式 SQL 初始化和公共收尾，不操作部署库、业务 topic／位点或其他 Worker 的环境。
- [x] **PS06.2 补齐真实 workflow 夹具的必要入口与断言。** 在[现有 RealHarness](../../crates/backend/tests/support/real.rs)中支持显式题号、finished／failed 两类终态和既有结果读取，保持登录、HTTP 创建、Scheduler、Kafka、真实 Worker／Isolate 与 MySQL 的实际路径。现有 real_http_worker_acceptance 的 completed 仅打印预期值；改为从测试专属 events topic 的真实终态消息核对 total_cases／completed_cases，并与持久化及 HTTP 结果对应。观察器不参与业务消费组或提交其位点；不为测试增加 HTTP 字段或新的生产抽象。
- [x] **PS06.3 完成两道正式题的真实业务验收。** 依赖 PS06.2；扩展 [real_http_worker_acceptance](../../crates/backend/tests/workflow.rs)，从三个正式 SQL 初始化的数据通过 HTTP 提交两数求和与商题的 C／Python 样例，分别核对 AC、104／8 例完成、固定题目版本以及 MySQL／HTTP 结果一致。保留现有 CE、WA、TLE 的代表场景和首败详情断言，检查 WA 仍返回用户输出和资源；脚本直接来自数据库装配，链路不注入受控终态。
- [x] **PS06.4 完成专用多解与 checker 故障的真实业务验收。** 依赖 PS06.2；仅在独立测试库增加题号 3 的专用多解夹具，使用 sum-pair.py、空参考和 {}，验证至少两份不同合法答案为 AC、非法用户输出为 WA。再以独立测试版本设置一个先 AC、后因非法参考形成 CheckerFailed 的代表场景，核对真实终态的先前完成数量、failed／无 verdict、稳定有界诊断及 MySQL 保存；私有参考或脚本诊断标记不进入 HTTP。复用正式装配、执行和保存入口，不增加第三道正式种子题。
- [x] **PS06.5 核对历史、私有数据与审查合同。** 依赖 PS06.3–PS06.4；从真实提交读取本人历史、详情和原始源码，核对题号／版本、AC／WA／failed 状态及原用户源码，公开题目和结果不包含 checker 源码、参数或 checker 原始 stdout／stderr。复用[审查请求合同](../../crates/backend/tests/llm_review.rs)与[审查 workflow](../../crates/backend/tests/cases/review_workflow.rs)，确认 LLM 输入仍为既有五项公开数据，failed 提交按既有合同拒绝审查；只有实际接线缺口才补断言，不重复外部模型效果评估。
- [x] **PS06.6 完成必要页面定向查看。** 依赖 PS06.3–PS06.5；复用现有前端和浏览器／桌面入口，查看两题题面、一次商题 AC、一次用户 WA、一次 CheckerFailed 及本人历史／源码。确认轮询进入终态、失败与 verdict 分开展示、用户输出正确转义、私有诊断不泄漏，保留用户已确认的“峰值内存”等当前页面文案及已有计量说明。复用结果组件和轮询检查，不新增页面、管理功能或浏览器测试框架；页面仅使用受控数据时如实注明，不能记作真实业务链路。
- [x] **PS06.7 完成必要回归并登记交付。** 依赖 PS06.3–PS06.6；按第 14.3 节执行实际改动需要的格式、编译、Clippy 和相关测试，新增 Rust 项补齐 Rustdoc。更新本记录的 CV01–CV08 实际覆盖、通过／失败／未测范围及任务状态；使用说明仅在现行功能或命令改变时同步。必要验收确实通过后才勾选 PS06；环境缺失和失败保留为待办，不用受控结果替代真实验收。

执行顺序为 PS06.1 → PS06.2 → PS06.3／PS06.4 → PS06.5／PS06.6 → PS06.7；共享同一对 box ID 的真实检查串行进行。完成条件是 CV01–CV08 均有对应的实际验证，正式预设、专用多解和 checker 故障完成真实跨服务路径，结果／历史及 LLM 输入合同保持，必要页面经过查看，失败和未测范围可明确判断。

### 2.7 PS07 四种通用预设脚本

定义、执行顺序和完成条件见[设计第 9.3 节](../13-checker-scripts-design.md#93-ps07-脚本与逻辑检查)。

- [x] **PS07.1 实现 tokens.py。**
- [x] **PS07.2 实现 integers.py。**
- [x] **PS07.3 实现 unordered-tokens.py。**
- [x] **PS07.4 实现 unordered-lines.py。**
- [x] **PS07.5 完成定向检查与材料说明。**

### 2.8 PS08 例题、种子与调用方

依赖 PS07；定义、16 例固定数据及完成条件见[设计第 9.4 节](../13-checker-scripts-design.md#94-ps08-例题种子与调用方)。

- [x] **PS08.1 装入正式脚本与例题。**
- [x] **PS08.2 提供八份提交样例。**
- [x] **PS08.3 适配现有夹具与列表。**
- [x] **PS08.4 验证初始化、装配和样例。**
- [x] **PS08.5 同步说明并收口阶段。**

### 2.9 PS09 真实链路验收与交付

依赖 PS07、PS08；定义、四份错误提交及完成条件见[设计第 9.5 节](../13-checker-scripts-design.md#95-ps09-真实链路验收与交付)。五项分解均已完成，实际结果见第 22 节。

- [x] **PS09.1 扩展正式题真实链路。**
- [x] **PS09.2 补四种代表性错误答案。**
- [x] **PS09.3 核对读取与专用场景。**
- [x] **PS09.4 执行串行真实验收。**
- [x] **PS09.5 同步说明并登记交付。**

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

## 10. PS04 实施记录

### 10.1 统一 checker 与输出边界

在 Worker 增加 checker.result_max_bytes = 4096 和 diagnostic_max_bytes = 65536 及正值校验。输出收集器复用原异步排空流程：checker stdout 超限停止，stderr 超限只截断保存并继续排空，每例新建收集状态。

[checker 模块](../../crates/worker/src/checker.rs)只接受单个完整 UTF-8 JSON 对象及唯一 verdict 字段，值限定为 AC／WA。通过保留重复字段证据的类型反序列化拒绝缺失、重复、未知字段和非法值，消费完整输入；额外限定对象形状，避免结构体反序列化接受数组。允许 JSON 首尾空白。

[执行器](../../crates/worker/src/executor.rs)移除内置 trim，全部正常用户运行均交给任务中的脚本，解释器取配置，参数固定为 -I -B -S checker.py input.bin answer.bin output.bin config.json，stdin 为空。CE 或用户已确认 MLE／OLE／TLE／RE 跳过 checker。无信号、无限额触发且实际退出码为 0 后，才解析完整 stdout；提前输出 AC 后非零退出仍为故障。

脚本语法、异常、非零退出、独立限额和协议错误统一形成 CheckerFailed；初始化／启动／停止／收集等错误沿用相应故障，JobDeadlineExceeded、WorkerShutdown 和 StopUnconfirmed 保持既有边界。公开诊断只含用例序号和稳定原因；内部英文日志保留实际退出、限额、截断事实及最多 4096 UTF-8 字节的 stderr 摘要。摘要在非法字节替换后计量；停止未确认的错误上下文不再拼接原始 stderr，防止通过故障诊断泄露脚本内容。

checker 取得 AC／WA 后才推进完成数量，自身故障保留先前数量；WA 首败短路并保留用户输出、退出事实与资源。用户和 checker 不合并资源。后端复用既有非空故障码校验、failed 保存和首个有效终态合同，没有增加结果类型或白名单。HTTP 不返回 checker 原始输出，机器 failed 不进入 LLM 审查，LLM 继续使用原五字段输入。

Rustdoc、TypeScript 资源类型及 Worker／后端／前端设计和使用说明已同步：CPU、墙钟为本次用户运行，内存为用户环境历史峰值，可能包含此前用例，均排除 checker 消耗。提交前用户确认保留结果组件的“峰值内存”等现有文案，不加入上一轮的页面说明；计量语义由类型与文档说明。保留原字段、单位、文本转义以及缺失和显式零的区别。根双语说明同步 checker 功能和输出预算。

### 10.2 实际检查

- cargo fmt --all -- --check、cargo check --workspace --all-targets --locked、cargo clippy --workspace --all-targets --locked -- -D warnings 通过。
- cargo test -p acorn-worker --locked：34 项常规测试通过，9 项外部环境检查在常规命令中 ignored。覆盖严格 JSON、退出事实、诊断 UTF-8 摘要、两类输出预算、实际固定参数及文件、跨例复用、WA 用户统计、首例及后续 checker 故障、完成数量、期限、停机和停止未确认；原 CE、用户错误、首败和输出限额检查继续通过。
- 真实 Isolate 的 real_checker_acceptance 和 real_executor_acceptance 串行执行，各 1 项通过。实际验证 checker 独立 CPU／墙钟／内存限额、三项缺失时的 Job 总期限、提前 AC 后非零退出，以及同用户环境先前用例产生的历史内存峰值；原 C／Python trim 脚本裁决继续通过。使用预先确认空闲的专用 ID 220／221，测试拥有的环境完成清理。
- 后端 lib 22 项、config_results 10 项通过；llm_review 的 request_contract_and_valid_opinions 通过，受控 HTTP 端点核对原五字段输入，没有调用外部模型。
- 真实 MySQL 的 terminal_storage_and_immutability 通过。真实 Kafka／MySQL 加受控终态夹具的 terminal_event_details、terminal_replay_bypasses_capacity_and_preserves_first_result 和 manual_http_rejections_preserve_records 各 1 项通过，覆盖 CheckerFailed 保存、公开 DTO、首个有效终态和审查拒绝；该夹具不执行真实 checker。
- web 中 pnpm format、format:check、lint、typecheck、build 和 pnpm test src/components/SubmissionOutcome.test.ts 通过，4 项既有组件渲染回归继续验证缺失／显式零、单位、输出转义和执行失败。构建有 CodeEditor 大包体积提示，构建成功；本阶段未新增静态文案镜像测试。

真实 Isolate 定向命令：

```sh
ACORN_ISOLATE_BOX_ID=220 ACORN_CHECKER_BOX_ID=221 \
  cargo test -p acorn-worker --locked --test executor -- --ignored --nocapture --test-threads=1
```

后端环境沿用第 4.3 节的独立库和临时 Kafka 资源合同。实际筛选命令：

```sh
cargo test -p acorn-backend --locked --lib
cargo test -p acorn-backend --locked --test config_results
cargo test -p acorn-backend --locked --test llm_review request_contract_and_valid_opinions
cargo test -p acorn-backend --locked --test database terminal_storage_and_immutability
cargo test -p acorn-backend --locked --test workflow terminal_event_details
cargo test -p acorn-backend --locked --test workflow terminal_replay_bypasses_capacity_and_preserves_first_result
cargo test -p acorn-backend --locked --test workflow manual_http_rejections_preserve_records
```

9 份变更 Markdown 通过 Prettier，266 个本地链接有效，其中 109 个含锚点，162 个原有标题按顺序保留；代码围栏闭合，6 项主任务和 28 项分解的编号／状态一致（4 项主任务、28 项分解完成）。根双语说明的 15 组命令／配置代码块及互链有效；git diff --check 通过。

### 10.3 检查修正与剩余范围

扩展 Worker 回归首次在并发运行时，300 ms 的 checker 期限检查得到 StopUnconfirmed，单独执行则通过。该检查改为 2 秒总期限，停机请求等待受控 checker 的启动标记后再发送，随后常规回归通过；生产停止流程未增加重试或降级。

存储测试首次直接访问 TerminalOutcome 私有字段导致编译失败，改为核对公开读取结果后通过。最初自选测试库前缀被 MySQL 1044 拒绝，数据库与 workflow 改用既有测试配置规定的 acorntest 前缀后通过，未扩大账户权限。LLM 输入检查首次因执行沙箱禁止本地监听失败，使用本地受控端点在获准环境重跑通过。Clippy 首次指出输出收集器测试模块后仍有实现项，调整模块顺序后通过，未添加 lint 豁免。

PS04 及 8 项分解已完成，PS05–PS06 保持未完成。本阶段分别验证真实 checker／Isolate 与后端消息保存及 HTTP 合同；完整 Kafka → Worker／真实 checker → MySQL 链路仍由 PS06 验收，未将受控夹具记作该链路通过。浮点预设、新增商题、专用多解夹具与完整初始化使用材料由 PS05 接续。本轮未运行浏览器实际页面、外部模型或完整部署验收。PS04 按用户要求单独提交，结果组件保留用户确认的现有文案。

## 11. PS05 定向验证安排

以下为 PS05 的定向验证安排，实际结果见第 13 节。优先扩展现有检查；不为预设另建一套执行框架。

### 11.1 脚本与文件协议

| 复用入口                                                                                                           | 覆盖与必要性                                                                                                                                                                                                       |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [Worker executor](../../crates/worker/tests/executor.rs)及[共享任务夹具](../../crates/worker/tests/support/mod.rs) | 直接使用编写材料中的脚本，核对 trim 的末尾 ASCII 空白、内部空白差异与原始非 UTF-8 字节，避免种子和测试各自维护不同规则。沿用 PS04 的退出、JSON、输出预算和完成数量检查。                                           |
| 同一 executor 中的浮点分组检查                                                                                     | 接受小数／科学记数法及连续 ASCII 空白；验证绝对与相对容差的内／外值、默认和显式配置。用户项数不符、非法数及 NaN／无穷大为 WA；非法参考数、未知或非法容差为 CheckerFailed。只覆盖上述明确合同，不展开参数笛卡尔积。 |
| 同一 executor 中的多解与故障检查                                                                                   | 相同 n 的两份不同合法答案均 AC，非法答案 WA；输入与空参考通过固定文件接口交接。浮点参考数据／参数错误经实际脚本执行形成 CheckerFailed，保持公开诊断和先前完成数量规则。                                            |
| 现有真实 Isolate 执行器入口                                                                                        | 使用实际脚本、C／Python 商题提交样例和专用多解夹具运行；核对真实退出与判题结果，验证 .py 单文件可在固定命令下工作。使用部署分配的两个空闲 ID 串行执行，避免同 ID 并发检查互相干扰。                                |

容差检查选用少量可明确判断的值。例如默认绝对容差下，参考 0、用户 1e-6 应通过，用户 2e-6 应 WA；仅启用相对容差 1e-6 时，参考 1000000、用户 1000000.5 通过，用户 1000002 为 WA。布尔值、负值、非有限值、rel_tol >= 1、错误类型和未知键按设计归为配置错误；无需重复 PS04 的完整 JSON 拒绝场景。

### 11.2 正式种子与调用方

| 复用入口                                                                                                                  | 覆盖与必要性                                                                                                                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [database](../../crates/backend/tests/database.rs)及[正式 SQL 初始化夹具](../../crates/backend/tests/support/database.rs) | 从空库执行原三条 SQL，接受结构版本 5；核对题号 1 的 104 例及题号 2 的 8 个连续用例、两种语言预算、两份脚本引用与容差参数。实际查询 MySQL 并与 .py 字节比较，检查 SQL 字符串转义没有改写源码；通过既有创建／装配入口取得完整商题任务。 |
| published_queries_and_fixed_version_creation、invalid_task_data_and_foreign_keys 与受影响的 database 场景                 | 更新旧列表数量和临时 ID 假设，保留固定版本、共享脚本、字段／外键约束和装配副本的原测试含义，避免新正式题号与既有夹具冲突。                                                                                                            |
| [workflow](../../crates/backend/tests/workflow.rs) 的 published_queries_and_history_shapes 及实际受影响场景               | 已发布列表与题面包含新商题，公开字段仍不返回 checker 私有数据，原题的版本及历史查询保持正常。受控终态只能证明 HTTP／保存合同，真实消息和沙箱联调在 PS06 汇总。                                                                        |
| [部署初始化入口](../../scripts/deploy.py)和使用说明                                                                       | 核对 SQL 执行顺序及安装文件保持一致，初始化提示准确描述两道题；文档中的题号、输入范围、容差、语言预算、样例和部署内容一致。无需为提示文案增加镜像测试。                                                                               |

8 个商题用例采用 (0,1)、(1,3)、(-1,3)、(1,1000000)、(-1,1000000)、(1000000,1)、(-1000000,1)、(1000000,3)，按上述顺序从 1 编号；参考值按题目定义保存为单个有限数值。两数求和原始用例顺序及字节保持不变。专用多解夹具使用 n = 5，以 1 4、2 3 和 1 3 区分两份合法输出与非法输出，不作为第三道正式预置题。

### 11.3 命令、环境与完成边界

Rust 实施包含测试夹具调整时，沿用现有检查：

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p acorn-worker --locked
```

数据库和 workflow 从既有 cargo test -p acorn-backend --locked --test database／--test workflow 入口选择实际受影响用例；新增检查在实施时确定名称并记录实际筛选命令。环境继续采用第 4.3 节的 ACORN_TEST_DATABASE_CONFIG、MYSQL_PASSWORD 和独立测试库合同，以及临时 Kafka topic／消费组。仅调整脚本、种子与示例的范围不重复外部模型、前端应用构建或完整部署验收。

真实执行复用[现有 Worker 命令](../../crates/worker/README.md#isolate-阶段检查)，扩展相应测试后执行：

```sh
ACORN_ISOLATE_BOX_ID=220 ACORN_CHECKER_BOX_ID=221 \
  cargo test -p acorn-worker --locked --test executor -- --ignored --nocapture --test-threads=1
```

这里的数字仅为示例，实施前必须由调用方分配两个空闲且专用的 ID。文档检查使用项目现有 Prettier，核对本地链接、稳定编号及根双语功能／命令一致性。预设通过受控和真实执行、SQL 通过真实初始化后，才能登记相应覆盖；CV01–CV03 的完整业务验收及 CV08 的真实链路仍按 PS06 实际结果收口。

## 12. PS05 规划记录

PS04 按用户要求单独提交为 d582852，提交前用户确认保留结果组件的原有“峰值内存”等文案，相关设计与实施描述已按该决定修正。

本轮补充 PS05 的 7 项分解、依赖、完成条件及定向验证安排。确定沿用现有初始化 SQL，保留题号 1 与 104 例，增加题号 2 的浮点商题及 8 例，专用多解作为开发夹具；明确题号 2 与现有未发布／临时题夹具冲突的同步工作。预设编写材料与数据库源码一致性由真实 MySQL 查询核对，运行继续采用 PS04 的统一 checker。

规划文档通过 Prettier，85 个本地链接有效，其中 11 个含锚点，27 个原有标题按顺序保留；主章节、任务编号、代码围栏及完成状态一致，PS05 的任务依赖无环，git diff --check 通过。

本轮未修改或执行 PS05 程序、SQL、配置和功能测试；PS05 的 7 项分解及 PS06 均保持未完成，规划文档保留为未提交修改。

## 13. PS05 实施记录

### 13.1 预设、正式种子与夹具

新增独立的 [trim.py](../../examples/checkers/trim.py)、[float.py](../../examples/checkers/float.py) 与 [sum-pair.py](../../examples/checkers/sum-pair.py) 编写材料，均使用标准库和固定四文件协议。trim 保留末尾六种 ASCII 空白语义及内部原始字节。float 先验证容差和参考数据，随后按项数、有限值及 math.isclose 判断用户输出；非法参考／配置形成 CheckerFailed，非法用户输出为 WA。sum-pair 使用原始输入和用户输出判断两正整数之和，参考为空。

现有 [002-example-problem.sql](../../sql/002-example-problem.sql) 在同一 DML 事务中写入两份预设及两道正式题，结构版本仍为 5。原题号 1／版本 1、脚本 ID 1 和 104 例的字节与顺序保持；新增脚本 ID 2、题号 2／版本 1 商题及 8 个固定用例，题面公开数值格式和两项 1e-6 容差。两题各语言预算一致，checker 覆盖值为空，沿用 Python 默认来源。正式 SQL 的脚本与 .py 保持字节一致；不将专用多解材料作为第三道正式题。

新增 [C 商题样例](../../examples/submissions/quotient.c)与 [Python 商题样例](../../examples/submissions/quotient.py)，分别输出固定小数和科学记数法。Worker 默认任务直接嵌入 trim.py，定向预设检查共享同一执行入口，避免再维护测试专用比较器。数据库旧临时题改用题号 3，列表、固定版本、外键及公开题面夹具同步保留原意，测试对原题数据的修改限定到对应题号。

部署脚本保持原三条 SQL 的执行与安装顺序，更新两题初始化提示。新增[编写材料说明](../../examples/checkers/README.md)，提供本地四文件运行和十六进制源码 SQL 更新示例；同步 SQL、数据库设计、脚本设计、Worker／后端说明、文档索引和根双语 Quick Start。文档明确数据库为运行时源码来源、共享引用与已装配副本语义。结果组件保持用户已确认的现有文案。

### 13.2 实际检查

- cargo fmt --all -- --check、cargo check --workspace --all-targets --locked 和 cargo clippy --workspace --all-targets --locked -- -D warnings 通过。
- cargo test -p acorn-worker --locked：36 项常规检查通过，10 项外部环境检查在该命令中 ignored。新增浮点预设与样例／多解检查，扩展原始二进制 trim 覆盖，继续通过既有用户错误、JSON、预算、首败、完成数量与停止合同。浮点分组覆盖默认／显式容差、边界内外、零容差、格式及非有限值、非法参考和参数；非有限容差使用脚本层直接任务夹具验证，公共接纳解析器仍可能先拒绝不可表示的 JSON 数值。
- 真实 Isolate 的 real_preset_acceptance 通过：C／Python 商题样例分别完成 8 例，sum-pair 两份不同合法输出均 AC，错误和非正数／非法格式输出为 WA。使用预先确认空闲的专用 ID 220／221 串行运行；执行器完成公共清理。该检查验证实际脚本、用户程序与隔离执行，不代表 Kafka／MySQL 整体链路。
- 真实 MySQL 的 checker_presets_and_quotient_seed、published_queries_and_fixed_version_creation、invalid_task_data_and_foreign_keys 各 1 项通过。实际从正式 SQL 初始化独立库，验证两份脚本的字节一致性、商题参数与预算、8 例顺序及任务装配，保留旧版本和外键检查；原两数求和 104 例的顺序及字节另经差异核对保持一致。
- workflow 的 published_queries_and_history_shapes 通过。实际 Kafka／MySQL 配合受控终态夹具验证两道题的公开查询、私有字段边界及原题版本／历史；此场景不执行真实 checker。
- 新说明中的本地四文件命令实际返回 AC；SQL 维护示例作为手动操作说明，未对部署库执行源码更新。
- 10 份 Markdown 通过 Prettier 检查；288 个本地链接（含 84 个锚点）有效，142 个原有标题保留，代码围栏闭合，主章节编号连续。15 组双语命令／配置块保持同步；任务清单为 5 项完成、1 项待办，35 项分解全部完成。git diff --check 通过。

实际定向命令如下，数据库及 Kafka 环境沿用第 11.3 节的独立测试资源合同：

```sh
cargo test -p acorn-backend --locked --test database checker_presets_and_quotient_seed
cargo test -p acorn-backend --locked --test database published_queries_and_fixed_version_creation
cargo test -p acorn-backend --locked --test database invalid_task_data_and_foreign_keys
cargo test -p acorn-backend --locked --test workflow published_queries_and_history_shapes
ACORN_ISOLATE_BOX_ID=220 ACORN_CHECKER_BOX_ID=221 \
  cargo test -p acorn-worker --locked --test executor real_preset_acceptance -- --ignored --nocapture --test-threads=1
```

### 13.3 检查修正

首次整组 Worker 回归中，原有 deadline_stops_execution_and_reports_failure 的 300 ms 期限在运行进程启动前到达，JobDeadlineExceeded 归因成立，但“已经启动一次运行”的断言失败。该用例的期限改为 2 秒，以保留运行中停止及禁止后续用例的覆盖；随后 36 项常规检查通过。生产阶段的停止、期限与占用逻辑未作调整。

### 13.4 交付边界

PS05 及 7 项分解完成，PS06 保持未完成。已交付两种预设、两道正式种子题、两份商题提交和专用多解材料。新增数据、脚本和既有调用方经过定向验证；完整真实 Kafka → Worker／checker → MySQL、必要页面及其余验收由 PS06 汇总，不把受控终态或单独 Worker 的结果记作完整业务闭环。

本轮未运行全部后端测试、前端构建／浏览器、外部模型或完整部署验收，未执行 Git 提交。

## 14. PS06 定向验收安排

### 14.1 已有覆盖与待补范围

下表保留 PS06 规划时的已有覆盖与缺口，实施结果见第 16 节。新增或扩展具体检查前，继续向用户说明覆盖内容及必要性。

| 验收                          | 已有结果                                                                                                         | PS06 补齐或核对                                                                                                          |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| CV01 trim／两数求和           | 第 9、10、13 节已验证真实 C／Python trim 裁决及原始字节语义；原题 104 例顺序与字节保持                           | PS06.3 让正式两数求和 C／Python 各完成 104 例真实业务链路，核对实际完成数量                                              |
| CV02 浮点                     | 第 13 节已验证容差内外、格式／项数、非有限数及参考／参数归因，真实商题 C／Python 各 8 例 AC                      | PS06.3 将正式商题的两种输出格式接入真实 HTTP → Kafka → Isolate → MySQL，不再重复整组非法参数矩阵                         |
| CV03 专用脚本与错误归因       | 第 13 节已有 sum-pair 的真实多解／WA，第 10 节已有 CheckerFailed 与完成数量定向覆盖                              | PS06.4 通过数据库装配专用脚本，并验证多解、用户 WA 以及先 AC 后 checker 故障的持久化路径                                 |
| CV04 退出、JSON 与诊断        | 第 10 节已有严格 JSON、结果超限、stderr 截断及真实“提前 AC 后非零退出”检查                                       | PS06.4–PS06.5 核对真实 checker 故障进入 failed／无 verdict，稳定诊断与私有原文边界；仅在受影响时复验原执行用例           |
| CV05 共享源码与副本           | 第 6 节真实 MySQL 与消息编码检查已验证共享引用、版本参数、旧副本不变及重新装配读取当前源码                       | 汇总现有 published_queries_and_fixed_version_creation 结果；任务装配或编码发生修改时定向复验，不增加源码历史或版本锁     |
| CV06 限额与总期限             | 第 6 节已验证覆盖优先级／缺失；第 10 节真实 Isolate 已验证 checker 独立 CPU／墙钟／内存及三项缺失时的 Job 总期限 | 核对 checker_limits_and_task_budgets 与 real_checker_acceptance 对应覆盖；仅在装配或执行限额受影响时复验，不扩展故障矩阵 |
| CV07 复用、隔离与资源         | 第 9 节真实双环境文件／生命周期、第 10 节真实历史峰值已有结果；受控检查覆盖公共停止与收尾                        | PS06.3–PS06.4 的多例、连续提交验证真实整合路径，结束后核对测试拥有资源的公共收尾；保留受控与真实证据边界                 |
| CV08 预算、完整链路与现有接口 | 第 6 节已有任务与编码预算；第 10、13 节的 HTTP／保存检查使用受控终态，尚未完成真实完整链路                       | PS06.3–PS06.6 补齐真实链路、历史／源码、LLM 投影与页面查看；真实 Kafka 编码往返复用现有测试入口                          |

### 14.2 联调组织与观察点

以 [real_http_worker_acceptance](../../crates/backend/tests/workflow.rs)及 [RealHarness](../../crates/backend/tests/support/real.rs)为主要入口，保留生产 HTTP、CreationService、Scheduler、KafkaWorker、真实 Isolate 和正式数据库装配。多解与非法参考只写入本例独立库；共享的正式种子不作运行中修改。新场景按实际规模放在现有测试目标中，不引入通用验收框架。

真实终态观察使用测试专属 events topic 的独立只读消费者，按 job_id 对应提交，读取实际 total_cases／completed_cases 和结果分支，不向测试链路发布替代终态。持久化仍经 Scheduler 的首个有效终态路径完成。HTTP 没有用例完成数字段，不能把测试中的预期变量、日志打印或 HTTP 轮询次数当成实际完成数量，也不因此扩充公开 DTO。

正式样例分别核对题号 1／2、版本 1 和对应 104／8 例；CE 为零例，首例用户 WA／TLE 为一例。故障夹具至少包含一例先前 AC，后例 checker 故障时 completed_cases 保留一例，state 为 failed、verdict 为空，failure code 为 CheckerFailed；故障稳定诊断继续受现有 512 UTF-8 字节上限约束。历史、详情和源码从同一真实提交读取，对比 MySQL 保存结果及原用户源码。

LLM 客户端使用现有受控 Responses 端点检查请求五字段及私有数据边界；failed 的自动／手动审查行为复用既有拒绝及交接检查。该结果证明输入与流程合同，不代表外部模型效果。页面查看使用现有前端入口与测试拥有的数据；真实接口和受控页面数据分别记录。

### 14.3 命令与收口边界

实施 Rust 夹具或断言修改时执行既有命令：

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p acorn-worker --locked
cargo test -p acorn-backend --locked --lib --test config_results
```

真实业务链路沿用[后端验收命令](../../crates/backend/README.md#检查命令)，执行前已提供 ACORN_TEST_DATABASE_CONFIG、MYSQL_PASSWORD 和 ACORN_TEST_KAFKA_SERVERS，并确认本例两个 box ID 空闲且专用：

```sh
ACORN_ISOLATE_BOX_ID=220 ACORN_CHECKER_BOX_ID=221 \
  cargo test -p acorn-backend --locked --test workflow real_http_worker_acceptance -- --ignored --nocapture --test-threads=1
```

上述 ID 仅为示例，不能把先前空闲视为本轮已保留。若专用多解／故障形成新的测试函数，命令在实现后按实际名称登记，不预先声称已执行。真实 MySQL 和 Kafka 资源的创建、权限与清理沿用既有测试合同，不修改服务器全局配置。

数据库、workflow 及 LLM 使用现有 cargo test -p acorn-backend --locked --test database／workflow／llm_review 入口按第 14.1 节选择相关场景；协议及真实 Kafka 预算检查复用[现有协议生成与检查](../../crates/protocol/README.md)和[后端 Kafka 检查](../../crates/backend/README.md#检查命令)。未改变的独立限额、隔离及错误矩阵使用已有结果；只有新修改、实际失败或未解决缺口才增加重跑。

页面定向验收前复用现有结果、题面和轮询测试，按需执行 web 的 pnpm test；实际修改前端代码时再执行既有 pnpm format:check、lint、typecheck 和 build。浏览器查看不能由组件测试替代；外部模型效果、双 Worker 并发、二进制故障矩阵及完整部署不属于本次新增验收范围，除非具体修改影响这些合同。

最后核对文档格式、稳定编号、本地链接与受影响的双语说明，使用实际命令和结果更新验收表。PS06 勾选须以必要验收通过为依据；缺少 MySQL、Kafka、Isolate 或页面入口时，记录对应待办，继续不依赖该环境的工作。

## 15. PS06 规划记录

PS05 按用户要求提交为 cb67620（feat: add checker presets and quotient examples），包含两份预设、多解材料、正式商题与样例、种子及调用方适配、实际检查记录和使用说明。

本轮补充 PS06 的 7 项分解、依赖、CV01–CV08 已有覆盖及剩余验收。确认现有真实夹具固定题号 1、仅支持 finished，完成数量仅打印预期值；实施时只补齐显式题号、failed 终态和实际事件断言。两道正式题、多解与 checker 故障通过同一真实跨服务路径收口；历史／源码、私有数据、LLM 输入及必要页面分别登记。

规划文档通过 Prettier、104 个本地链接（含 14 个锚点）和 git diff --check 检查；38 个原有标题按原顺序保留，主章节 1–15 连续，代码围栏闭合，CV01–CV08 与任务编号有效。主任务为 5 项完成／1 项待办；原 35 项分解保持完成，新增 PS06 的 7 项分解均为待办。

本轮仅提交既有 PS05 改动并编写 PS06 规划，未修改或执行 PS06 程序、SQL、配置或功能测试。PS06 及 7 项分解保持未完成，规划修改尚未提交。

## 16. PS06 实施记录

### 16.1 真实夹具与业务验收

扩展 [RealHarness](../../crates/backend/tests/support/real.rs)的显式题号和 finished／failed 终态检查。终态读取后，使用测试专属 events topic 的独立显式分区观察器按 job_id 取得真实消息，核对 Kafka key、状态、verdict／failure、total_cases 与 completed_cases，并与 MySQL 和 HTTP 结果对应。观察器不订阅消费组、不提交位点，释放后再完成既有公共收尾；夹具记录所拥有的库及 topic 身份以便失败后定位。

[real_http_worker_acceptance](../../crates/backend/tests/workflow.rs)通过真实登录、HTTP 创建、正式数据库装配、Scheduler、KafkaWorker 与 Isolate 执行两道题的 C／Python 样例。两数求和分别 AC／104 例，商题分别 AC／8 例；原 CE、首例 WA 和首例 TLE 分别完成 0、1、1 例，保留诊断、用户输出、退出事实和资源断言。完成数量取自实际终态，原先仅打印预期值的覆盖已替换。

新增同一 workflow 目标下的 real_checker_workflow_acceptance，仅在独立测试库增加题号 3 的开发夹具：版本 1 引用 sum-pair.py，同一 n = 5、空参考和 {} 对应输出 1 4、2 3、1 3，分别得到 AC、AC、WA。版本 2 引用正式浮点脚本，三例中的第二例保存非法参考；用户正常输出后，第一例 AC、第二例 CheckerFailed，实际完成数量为 1／3。HTTP 和 MySQL 保存 failed／无 verdict，诊断为“Checker failed for case 2: exit code 1.”，不包含私有参考标记、Python 异常原文或脚本内容。正式两题种子保持原样。

两项真实验收均读取对应提交的本人历史、详情、固定版本题面与独立源码接口，核对题号／版本、语言、状态、verdict、审查状态及原始用户源码。公开对象不带 checker 源码、引用、参数、限额、隐藏用例或参考输出；列表继续省略源码和终态详情。更新[后端检查说明](../../crates/backend/README.md#检查命令)的覆盖及专用脚本命令。

### 16.2 实际检查

| 命令／入口                                                       | 实际结果                                                                                                              |
| ---------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| cargo fmt --all -- --check                                       | 通过                                                                                                                  |
| cargo check --workspace --all-targets --locked                   | 通过                                                                                                                  |
| cargo clippy --workspace --all-targets --locked -- -D warnings   | 通过                                                                                                                  |
| cargo test -p acorn-worker --locked                              | 36 项常规检查通过，10 项外部环境检查在该命令中 ignored；覆盖脚本协议、预设、预算、进度、资源、期限及停止合同          |
| cargo test -p acorn-backend --locked --lib --test config_results | 22 项库测试及 10 项配置／结果测试通过                                                                                 |
| workflow 的 real_http_worker_acceptance                          | 1 项通过，实际包含正式两题 C／Python AC 及 CE／WA／TLE，共 7 次真实提交                                               |
| workflow 的 real_checker_workflow_acceptance                     | 修正第 16.4 节的夹具断言后 1 项通过，实际包含多解 AC／AC／WA 及先 AC 后 CheckerFailed，共 4 次真实提交                |
| workflow 的 manual_http_rejections_preserve_records              | 1 项通过；使用启用受控审查端点的既有夹具，failed 提交返回 409／JudgingNotFinished，记录不变，未向模型发出请求         |
| llm_review 的 request_contract_and_valid_opinions                | 1 项通过；受控 Responses 请求保持 source、language_id、title、statement、verdict 五字段，原源码字节和既有诊断边界保持 |
| kafka 的 real_kafka_message_roundtrip                            | 1 项通过；真实 broker 往返包含 checker 字段的大任务及事件，原始字节、编码和接收预算按既有合同检查                     |
| 前端现有 Vitest                                                  | 实际运行 17 个文件、108 项检查并通过，包含结果、题面、轮询、历史及源码相关检查                                        |
| 本地 Chromium 页面查看                                           | 通过，详见第 16.4 节；使用受控 HTTP 数据，与上述真实后端链路分开登记                                                  |

真实业务命令如下。MySQL、密码与 Kafka 地址通过既有测试环境传入；数据库采用独立前缀加 UUID，Kafka 使用本例专属 topic／group，两个预先确认空闲的 box ID 串行使用。

```sh
ACORN_ISOLATE_BOX_ID=220 ACORN_CHECKER_BOX_ID=221 \
  cargo test -p acorn-backend --locked --test workflow real_http_worker_acceptance -- --ignored --nocapture --test-threads=1
ACORN_ISOLATE_BOX_ID=220 ACORN_CHECKER_BOX_ID=221 \
  cargo test -p acorn-backend --locked --test workflow real_checker_workflow_acceptance -- --ignored --nocapture --test-threads=1
cargo test -p acorn-backend --locked --test workflow manual_http_rejections_preserve_records
cargo test -p acorn-backend --locked --test llm_review request_contract_and_valid_opinions
cargo test -p acorn-backend --locked --test kafka real_kafka_message_roundtrip -- --ignored --nocapture
```

前端实际调用 pnpm test -- src/components/SubmissionOutcome.test.ts src/components/ProblemStatement.test.ts src/composables/useSubmissionPolling.test.ts；该调用实际执行整组 17 个文件而非仅这三个文件，上表按实际结果登记。前端组件没有改动，本轮未新增前端测试。

### 16.3 CV01–CV08 验收汇总

| 验收 | 实际覆盖与结论                                                                                                                                                    |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| CV01 | 第 13 节的 trim 原始字节语义及本轮 Worker 回归保持；正式两数求和 C／Python 各 104 例真实完整链路 AC，通过                                                         |
| CV02 | 第 13 节和本轮 float_preset_contract 覆盖容差、格式／项数、非法数及参考／配置归因；正式商题的固定小数／科学记数法各 8 例真实完整链路 AC，通过                     |
| CV03 | 本轮真实数据库装配的专用多解为 AC／AC／WA；非法参考通过真实浮点 checker 形成 CheckerFailed，先前完成数量保持，通过                                                |
| CV04 | 第 10 节与本轮 Worker 回归覆盖严格 JSON、成功退出、输出预算及提前 AC 后故障；本轮真实 CheckerFailed 保存及私有诊断断言通过                                        |
| CV05 | 沿用第 6 节真实 MySQL 与消息副本检查：共享引用、版本独立参数、旧任务副本不变和重新装配取得当前源码；本轮未改装配／编码实现，已有覆盖保持                          |
| CV06 | 沿用第 6 节的限额来源／缺失和第 10 节真实 checker 独立限额／Job 总期限结果；本轮 Worker、配置及结果回归通过，未改生产限额实现                                     |
| CV07 | 第 9、10 节真实双环境、文件及历史峰值检查保持；本轮真实多例和连续 Job 执行通过，两个任务专用 box 均完成清理，停止及占用合同回归通过                               |
| CV08 | 本轮真实 Kafka 编码往返及两项完整 HTTP → Kafka → Worker／Isolate → MySQL 验收通过；真实历史／源码、受控 LLM 输入合同和必要页面查看通过，覆盖边界见第 16.4–16.5 节 |

### 16.4 检查修正与页面范围

专用脚本验收最初在额外手动审查断言失败：请求遗漏必填 expected_review_id 时返回 400；补齐后，该真实 Isolate 夹具因审查服务禁用而按合同返回 503，无法使用启用审查夹具的 409 预期。删除该处重复且不适用的断言，保留真实故障链路、完成数量、持久化和私有诊断检查；使用既有 manual_http_rejections_preserve_records 单独验证启用审查时的 failed 拒绝与零请求。生产审查实现未调整，最终真实专用脚本场景通过。

前两次断言失败留下的独立测试库及 Kafka topic／group，经本轮专有参考标记、脚本字节与实际事件 job_id 核对身份后清理。成功场景使用夹具公共收尾；任务所用两个 Isolate 目录已核对为空。没有增加自动扫描或恢复机制。

浏览器使用已有本地 Chromium 156、1280 × 900 视口和现有 Vite 页面，HTTP 数据受控，包含两道题面、商题 AC、用户 WA、CheckerFailed、本人历史及只读源码。三种结果均从 running 轮询到终态并停止继续轮询；WA 中的脚本文本按字面展示且未执行；failed 与机器 verdict 分开展示，历史无 verdict 显示“未产生”，原始源码无法编辑。私有参考与异常原文未进入页面，“峰值内存”当前文案保持，页面无脚本错误。最初页面脚本遗漏标题的题号前缀导致定位超时，修正定位后通过；未因此修改产品页面。

### 16.5 交付边界

PS06 及 7 项分解完成，PS01–PS06 全部完成。实际改动集中在真实测试夹具、验收用例、检查说明与交付记录；CV01–CV08 由已有定向结果和本轮真实链路共同收口。历史、结果和源码由真实 HTTP／MySQL 验证，LLM 输入由受控 Responses 端点验证，页面由受控 HTTP 数据验证，各自不扩大为其他层面的结论。

两份变更 Markdown 通过 Prettier，131 个本地链接（含 27 个锚点）有效，52 个原有标题按原顺序保留；主章节 1–16、代码围栏及两张 CV01–CV08 表有效，6 项主任务和 42 项分解均已完成，git diff --check 通过。前两次失败场景的独立库、topic／group 已核对不存在，两个任务专用 box 均为空。

本轮未重跑未受影响的完整数据库、独立限额／隔离矩阵，未运行双 Worker 并发、完整部署或外部模型效果验收。当前页面文案按用户确认保留。

## 17. 通用预设扩展规划记录

按用户要求在现有 checker 设计中追加轻量任务 PS07–PS09，交付目标为 tokens、integers、unordered-tokens、unordered-lines 四种脚本，以及单词逆序、整数前缀和、筛选单词、相邻单词对四道正式例题。约定整数精确比较、无序重复计数和行内记录关系；例题使用题号／脚本 ID 3–6，原临时夹具移至 7。

三份变更文档通过 Prettier，143 个本地链接（含 19 个锚点）有效，76 个原有标题按原顺序保留；章节、任务编号及代码围栏有效，原 CV01–CV08 内容保持，git diff --check 通过。

本轮仅修改 checker 设计、实施记录和索引导航。PS01–PS06 的完成状态保持，PS07–PS09 均为待办；未实施脚本、种子、样例或功能检查，未提交 Git。

## 18. PS07–PS08 任务细化记录

本轮在现有设计中补充 PS07、PS08 各 5 项任务和对应进度清单，固定四题的 16 例内容、八份提交样例及临时题／脚本 ID 7 的适配范围。整数脚本通过符号与前导零规范化保持任意精度，记录比较保留每行字段关系；本阶段同步材料和初始化使用说明，完整真实链路及受影响 ignored 场景仍由 PS09 验收。

三份变更文档通过 Prettier，150 个本地链接（含 21 个锚点）有效，76 个原有标题按原顺序保留；章节、代码围栏及设计／执行任务名称一致，原 CV01–CV08 保持，git diff --check 通过。原 42 项分解保持完成，新 10 项分解均为待办。

本轮仅细化规划；PS07、PS08 的 10 项分解和 PS09 均保持未完成，既有 PS01–PS06 状态不变。未修改程序、SQL、配置或样例，未运行功能检查，未提交 Git。

## 19. PS07 实施记录

### 19.1 脚本与材料说明

新增独立标准库材料 [tokens.py](../../examples/checkers/tokens.py)、[integers.py](../../examples/checkers/integers.py)、[unordered-tokens.py](../../examples/checkers/unordered-tokens.py) 和 [unordered-lines.py](../../examples/checkers/unordered-lines.py)。四份脚本先要求配置为 {}，随后只读取参考和用户输出；使用固定四文件参数、退出码 0 与唯一 JSON 对象返回 AC／WA，参数错误以非零退出形成 CheckerFailed。

tokens 按 ASCII 空白分词后有序比较原始字节；unordered-tokens 比较词项计数。integers 先完整验证参考的十进制词项，再解析用户输出，规范化符号与前导零，零统一处理，使用字节内容保持任意精度；参考非法形成 CheckerFailed，用户非法形成 WA。unordered-lines 仅以 LF 分行、行内按 ASCII 空白分词，忽略空白行，以字段元组计数，保留重复记录和字段关系。

在既有 [executor](../../crates/worker/tests/executor.rs)增加一个共用断言入口和四项分组测试，直接嵌入实际脚本源码，沿用受控管理器和真实 Python 子进程执行。更新[材料说明](../../examples/checkers/README.md)的比较规则、合法输出和本地四文件使用方式，并明确正式 SQL 已收录预设与独立材料的区别；设计中的四种材料增加源码入口。

### 19.2 实际检查

| 命令                                                           | 实际结果                                                                                                             |
| -------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| cargo fmt --all -- --check                                     | 通过                                                                                                                 |
| cargo check --workspace --all-targets --locked                 | 通过                                                                                                                 |
| cargo clippy --workspace --all-targets --locked -- -D warnings | 通过                                                                                                                 |
| cargo test -p acorn-worker --locked --test executor preset     | 6 项通过，1 项真实 Isolate 检查保持 ignored，16 项未被筛选；其中新增四项分组测试，原浮点预设及商题／多解样例继续通过 |

新增四项测试共运行 44 个场景：词项比较 18 例覆盖合法 ASCII 空白、有序／无序差异、大小写、缺少／多余项、重复计数、非 ASCII 空白和原始二进制；整数 14 例覆盖符号／前导零／负零、有序性、数量、非法格式及参考优先归因，包括 5000 位十进制整数的等价表示 AC 和末位差异 WA；记录比较 8 例覆盖 CRLF、空白行、行内字段及跨行配对、重复计数和仅 LF 分行；四份脚本分别拒绝非空配置。AC／WA 的 completed_cases 为 1，CheckerFailed 为 0。

四份变更 Markdown 通过 Prettier，172 个本地链接（含 22 个锚点）有效，80 个原有标题按原顺序保留；主章节、代码围栏及任务名称对应有效。7 项主任务及 47 项分解完成，PS08／PS09 与 5 项分解保持待办；原 CV01–CV08 内容不变，git diff --check 通过。

### 19.3 阶段边界

PS07 及其 5 项分解完成。四份材料使用现有命令与执行器，无需调整生产 Worker、协议、数据库结构或资源合同。本轮未改正式 SQL 或新例题数据；正式初始化仍为两题，四份新材料的入库、四题 16 例和八份提交由 PS08 完成，完整真实业务链路由 PS09 验收。

本轮检查证明脚本逻辑及执行器接线，受控管理器结果不作为真实隔离或资源限额证据。未运行真实 Isolate／MySQL／Kafka、浏览器、外部模型或完整部署检查；未重跑未受影响的完整测试集，未执行 Git 提交。

## 20. PS08 实施记录

### 20.1 正式例题与提交样例

扩展 [002-example-problem.sql](../../sql/002-example-problem.sql)，在原事务中写入脚本 ID 3–6，分别由同号题目版本 1 引用。四题为单词逆序、整数前缀和、筛选单词和相邻单词对，按设计第 9.4 节各装入四个固定有序用例，最后统一发布六题。题面写明输入范围及相应的空白、整数表示或顺序规则；参数均为 {}，checker 覆盖值为空，六题相同语言的预算一致，自动审查关闭。结构版本保持 5，原三条初始化顺序不变，原两题的 104／8 条输入及参考数据按原顺序保留。

新增八份提交样例，入口见[脚本说明](../../examples/checkers/README.md#通用比较材料)。逆序题的 C 一行输出、Python 每词一行；前缀和的 C 使用 long long，Python 使用整数并输出显式符号和前导零；筛选和数对的 Python 采用与 C 相反的合法顺序，保留重复项及行内字段关系。前缀和最后一例包含精确结果 9007199254740993，筛选第一例参考为零字节。

同步部署初始化完成提示、SQL 说明、数据库设计、Worker／后端说明、文档索引及根双语 Quick Start 的六题功能和样例入口；未改变现有页面文案。

### 20.2 夹具与调用方

扩展 [database](../../crates/backend/tests/database.rs)的既有种子检查，读回六份正式源码并按字节核对仓库文件；验证新四题的版本、标题、审查模式、脚本 ID、{} 参数、空覆盖值、四例连续序号，以及 C／Python 装配后的源码、有效预算和完整输入／参考顺序。固定版本与共享脚本检查中的临时题统一移至 7，保留未发布题不可提交、源码副本、参数校验与外键检查。

[workflow](../../crates/backend/tests/workflow.rs)的公开列表改为题号 1–6，检查新四题公开详情、语言和私有字段边界。真实专用场景中的 sum-pair 脚本及临时题同步移至 7，包括两个版本、用例、发布更新和提交引用；该 ignored 场景本阶段仅编译，留待 PS09 复验。

[executor](../../crates/worker/tests/executor.rs)复用既有样例入口，直接嵌入八份实际源码和对应 checker，分别执行四例并核对 AC／completed_cases=4。该入口仍保留商题 C／Python 各八例及五次多解输出判断，真实 Isolate 入口共享相同样例内容。

### 20.3 实际检查

| 命令                                                                                              | 实际结果                                                                            |
| ------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| cargo fmt --all -- --check                                                                        | 通过                                                                                |
| cargo check --workspace --all-targets --locked                                                    | 通过，包含受影响的 ignored 测试目标编译                                             |
| cargo clippy --workspace --all-targets --locked -- -D warnings                                    | 通过                                                                                |
| cargo test -p acorn-backend --locked --test database checker_presets_and_quotient_seed            | 1 项通过，15 项未被筛选                                                             |
| cargo test -p acorn-backend --locked --test database published_queries_and_fixed_version_creation | 1 项通过，15 项未被筛选                                                             |
| cargo test -p acorn-backend --locked --test database invalid_task_data_and_foreign_keys           | 1 项通过，15 项未被筛选                                                             |
| cargo test -p acorn-backend --locked --test workflow published_queries_and_history_shapes         | 1 项通过，44 项未被筛选                                                             |
| cargo test -p acorn-worker --locked --test executor preset_samples_and_multiple_answers           | 1 项通过，22 项未被筛选；八份新提交共执行 32 例，原商题 16 例及五次多解判断继续通过 |

数据库检查使用真实 MySQL 与正式三条 SQL 创建独立库，正常收尾关闭连接并删除本例库。公开查询使用既有 workflow 与专属 Kafka 测试资源，终态由受控 Worker 产生；样例入口运行真实 GCC／Python 子进程及受控管理器，不作为真实隔离证据。未发生失败检查。

静态核对原两题的 112 条输入／参考行及顺序与提交前一致，部署脚本和四份 Python 提交样例语法有效。变更 Markdown 通过 Prettier，本地链接／锚点、原有标题顺序、章节及任务编号、双语命令配置块与互链有效，原 CV01–CV08 内容不变；8 项主任务与 52 项分解完成，PS09 保持待办，git diff --check 通过。

### 20.4 阶段边界

PS08 及其 5 项分解完成。全新初始化包含六份正式预设及六道题，四题 16 例和八份提交可装配并通过受控执行器检查。PS09 继续验收新四题的完整 HTTP → Kafka → Worker／Isolate → MySQL、调整后的专用真实场景及联调产生的剩余说明。

本阶段未运行真实 Isolate、ignored 真实业务场景、浏览器、外部模型或完整部署；未重跑未受影响的完整测试集，既有真实链路结论不扩展到新四题。

## 21. PS09 任务规划记录

### 21.1 规划范围

PS08 已提交为 0a4fa60。本轮在设计第 9.5 节追加 PS09 的五项分解，并在第 2.9 节保持待办：扩展八份正式样例的真实业务链路，追加四种代表性错误提交，复用读取合同和 ID 7 专用场景，串行运行三个既有真实入口，最后同步检查说明与验收记录。

四份新错误提交分别覆盖顺序、2^53 以上的整数精度、重复计数和行内配对。固定正式数据及预期首败／完成数见设计中的表格，均为后续执行的断言目标。本轮未改测试、程序、SQL 或提交样例，未运行功能和外部服务检查，PS09 及其五项分解均未完成。

### 21.2 后续执行入口

实施时先准备 MYSQL_PASSWORD、ACORN_TEST_DATABASE_CONFIG、ACORN_TEST_KAFKA_SERVERS，以及明确预留且不同的 ACORN_ISOLATE_BOX_ID／ACORN_CHECKER_BOX_ID，再在仓库根目录依次执行以下既有入口。三个命令共享同一对 box ID，不能同时运行；独立库、topic／group 与正常清理沿用现有夹具。

```sh
cargo test -p acorn-worker --locked --test executor real_preset_acceptance -- --ignored --nocapture --test-threads=1
cargo test -p acorn-backend --locked --test workflow real_http_worker_acceptance -- --ignored --nocapture --test-threads=1
cargo test -p acorn-backend --locked --test workflow real_checker_workflow_acceptance -- --ignored --nocapture --test-threads=1
```

实施前按仓库要求说明每项新增检查的覆盖及必要性。Rust 改动完成后执行既有格式、编译、Clippy 与相关测试；真实入口发生失败时登记原因与修正，仅重跑受影响检查。八份新样例 AC、四种错误提交 WA、ID 7 多解／CheckerFailed 和读取合同实际通过后，才登记 PS09 完成。

### 21.3 本轮文档检查

两份变更 Markdown 通过 Prettier，本地链接／锚点、原有标题顺序、章节编号和代码围栏有效；设计与实施清单中的五项任务名称一致，原 CV01–CV08 保持。PS01–PS08 及原 52 项分解维持完成，PS09 与新增五项分解保持待办，git diff --check 通过。本轮规划未提交 Git。

## 22. PS09 实施记录

### 22.1 实际改动

扩展 [real_http_worker_acceptance](../../crates/backend/tests/workflow.rs)，通过正式 SQL 初始化独立库，再经 HTTP 提交题号 3–6 的八份实际 C／Python 样例。每份核对版本 1、AC 及真实 Kafka 的 total_cases=completed_cases=4；保留原两数求和、商题及 CE／WA／TLE。新增四份 Python 错误提交，使用正式数据验证设计第 9.5 节的顺序、整数精度、重复计数与行内配对，逐项核对首败序号、退出码 0、无截断、空 stderr 及用户 stdout。

复用 real_submission_reads 核对每份新增提交的本人历史、固定题目版本、原始源码与公开响应的私有字段边界，并增加 DISABLED／无审查结果断言。在 [RealHarness](../../crates/backend/tests/support/real.rs)补充真实 Kafka 终态与 MySQL／HTTP 详情的对应检查：首败的输出、截断标记、资源及退出状态一致，CE 编译诊断与截断标记一致，AC 无失败详情；checker 故障沿用已有诊断一致性断言。字段核对使用公开的 CaseDetails，不改生产接口。

更新 Worker／后端的真实检查说明，补充定向预设命令、六题样例、四种 WA、ID 7 专用场景及串行约束。生产程序、协议、数据库结构、脚本、正式种子、提交样例及页面保持原实现。

### 22.2 实际检查

三个真实入口按设计顺序使用同一对预留空闲 box ID 串行执行。后端检查从正式三条 SQL 创建独立 MySQL 库，使用专属 Kafka topic／group，经过实际 HTTP、Scheduler、Kafka、Worker／Isolate 与 MySQL 保存路径；事件观察器不加入业务消费组或提交位点。所有通过入口正常执行既有收尾，删除本次库、topic／group，两个任务专用 box 均为空。

| 命令                                                                                                                            | 实际结果                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| cargo fmt --all -- --check                                                                                                      | 通过                                                                                       |
| cargo check --workspace --all-targets --locked                                                                                  | 修正测试字段访问后通过，包含受影响的 ignored 目标                                          |
| cargo clippy --workspace --all-targets --locked -- -D warnings                                                                  | 通过                                                                                       |
| cargo test -p acorn-worker --locked --test executor real_preset_acceptance -- --ignored --nocapture --test-threads=1            | 1 项通过，22 项未被筛选；商题及新四题共十份 C／Python 样例完成 48 例，多解五次判断继续通过 |
| cargo test -p acorn-backend --locked --test workflow real_http_worker_acceptance -- --ignored --nocapture --test-threads=1      | 修正编译后 1 项通过，44 项未被筛选；共 19 次真实提交                                       |
| cargo test -p acorn-backend --locked --test workflow real_checker_workflow_acceptance -- --ignored --nocapture --test-threads=1 | 1 项通过，44 项未被筛选；ID 7 的多解与 checker 故障共四次真实提交                          |

正式业务入口中，八份新样例均完成四例 AC，共 32 例；合法换行、符号／前导零、无序记录与重复项均由数据库装配的对应脚本接受。两数求和 C／Python 各 104 例、商题各 8 例通过，原 CE、首例 WA／TLE 继续通过。四份新错误提交均为 WA，题号 3–6 的首败序号与完成数依次为 2、4、2、2，总数均为 4；整数首败 stdout 的末项实际为 9007199254740992，未被接受为参考中的 9007199254740993。词项顺序、重复 cat 缺失和数对 b a 改为 a b 分别按合同被拒绝。

ID 7／版本 1 的两份合法多解为 AC，错误输出为 WA，三次均完成 1／1。版本 2 第一例通过，第二例参考非法形成 CheckerFailed，真实终态 total_cases=3、completed_cases=1；HTTP／MySQL 为 failed、无 verdict，诊断有界且不包含私有参考、Traceback 或原始异常内容。本人历史、源码、固定版本及 DISABLED 审查断言全部通过。

### 22.3 检查修正

首次所有目标编译和首次 real_http_worker_acceptance 启动均因新增测试读取 TerminalOutcome.details 私有字段而在编译阶段失败；该次真实入口未进入运行，未创建测试服务或业务资源。改为反序列化公开 CaseDetails 并逐项比较 Kafka 首败数据，随后所有目标编译及真实入口通过。未放宽生产字段可见性或改变判题行为；本阶段无运行期失败与未解决检查。

### 22.4 阶段交付与范围

PS09 及其 5 项分解完成，PS01–PS09 和 57 项分解全部完成。原 CV01–CV08 表与历史结论保留；本轮为新四题补齐真实执行、完整业务链路、错误答案、持久化及读取合同，PS07／PS08 的受控检查仍按原范围记录。

四份变更 Markdown 通过 Prettier，本地链接／锚点、既有标题顺序、章节编号、任务定义与清单、代码围栏有效，git diff --check 通过。仅运行本轮变更需要的格式、编译、Clippy 和三个真实入口；未重跑完整常规测试集、独立协议／资源故障矩阵、双 Worker 并发、外部模型或完整部署。没有前端改动，未新增浏览器验收，页面文案按用户确认保持。

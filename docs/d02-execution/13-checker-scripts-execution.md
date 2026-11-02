# Python 判题脚本实施记录

对应[设计 v1](../13-checker-scripts-design.md)。任务沿用 PS01–PS06，验收沿用 CV01–CV08。

## 1. 当前状态

PS01 已完成结构版本 5、存储与初始化适配及相应检查，实际结果见第 5 节。PS02–PS06 尚未交付，任务分解见第 2.1、2.2 节；实际执行器继续使用原固定 trim 比较。

## 2. 任务状态

任务定义与依赖见[设计第 7 节](../13-checker-scripts-design.md#7-实施任务)。

- [x] PS01 数据结构与存储。
- [ ] PS02 协议、任务装配与预算。
- [ ] PS03 沙箱生命周期与文件对接。
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

- [ ] **PS02.1 扩展 Proto 并生成共享类型。** 在 [worker.proto](../../proto/acorn/worker/v1/worker.proto)为 SubmitJobRequest 增加字段号 6–8 的 checker_source、checker_config_json、checker_limits，新增含三个 optional uint64 的 CheckerLimits，保留现有字段号和结果结构。按[现有生成流程](../../crates/protocol/README.md#构建与生成)重新构建生成代码，并同步受影响的任务构造点；生成输出由 Cargo／Prost 管理。
- [ ] **PS02.2 装配脚本副本与有效限额。** 依赖 PS01、PS02.1；在[数据库任务装配](../../crates/backend/src/database/submissions.rs)按提交固定版本读取脚本源码、版本参数和覆盖值，以及该版本可选的 Python 语言限额。每项独立采用“题目覆盖 → Python 同项限额 → 不设限”，保留 NULL 与正值的区别，拒绝不能转换为沙箱数值的有限值。即使提交语言为 C，也从 Python 行取得 checker 默认值；缺少 Python 行不妨碍装配。脚本只在 Job 级携带，沿用短连接／事务和自有数据返回方式。
- [ ] **PS02.3 衔接全部装配入口与副本语义。** 依赖 PS02.2；核对新建、queued 幂等复用／重交接及启动装入均使用同一 load_task 合同。新建完成装配后将完整任务交给[创建服务](../../crates/backend/src/submissions/creation.rs)与 [Scheduler](../../crates/backend/src/scheduler/mod.rs)，发布端直接编码内存任务。dispatching、running 启动时仅恢复等待状态。内存任务和已编码消息保留装配时的源码与参数，确实需要重新装配 queued 时读取当前脚本。
- [ ] **PS02.4 接入后端配置与任务预算。** 在[后端配置](../../crates/backend/src/config.rs)及[示例](../../config/backend.example.toml)增加 request.checker_source_max_bytes = 65536 和 checker_config_max_bytes = 16384。装配时校验 checker 源码非空、UTF-8 和单项预算，校验参数为 UTF-8 JSON 对象并满足文本字节预算；合计预算在现有用户源码、用例输入和参考数据上计入这两项，沿用 checked_add 与错误报告。完整 Protobuf 继续通过现有 Kafka 编码预算检查，普通 HTTP DTO 和 LLM 输入保持既有投影。
- [ ] **PS02.5 接入 Worker 接纳与可选限额校验。** 依赖 PS02.1；同步 [Worker 配置](../../crates/worker/src/config.rs)、[示例](../../config/worker.example.toml)和[公共接纳入口](../../crates/worker/src/runtime.rs)的新增请求预算、UTF-8、JSON 对象和合计预算校验。checker_limits 消息必须存在，空消息表示三项均不设限；存在的有限值必须为正且可由沙箱表示。用户 RunLimits 仍必须为正并服从原 max_run 上限；checker 未设限项保持缺失。沿用 InvalidJob 接纳失败合同，Python 语法与运行错误留给执行阶段处理。
- [ ] **PS02.6 完成协议与装配定向回归。** 依赖 PS02.2–PS02.5；按第 4.2 节扩展现有协议、数据库、配置、Worker 和 workflow 检查，同步所有受影响的任务夹具。覆盖新增字段往返、限额缺失语义、共享脚本与版本参数、源码副本与重新装配、三类装配入口，以及新增字节预算和公开响应边界；复用原有幂等、固定版本和 Kafka 大消息检查，不重复完整故障矩阵。
- [ ] **PS02.7 同步合同并收口阶段交付。** 依赖 PS02.6；同步受影响的[Worker 设计](../01-worker-design.md)、[数据库设计](../02-database-design.md)、[后端设计](../03-backend-design.md)、[Kafka 设计](../10-kafka-judging-design.md)、协议及服务说明和两端配置示例。按第 4.3 节完成格式、编译、Clippy 与相关测试，在本记录登记实际通过、失败和未测范围；所有新写 Rust 项补齐 Rustdoc。

PS02.1 可在 PS01 数据库工作期间先推进协议定义；装配接线须在 PS01 就绪后完成。PS02.4、PS02.5 可以分别推进，随后完成调用方回归和阶段检查。完成条件为所有装配入口产生同一合同的完整任务，源码／参数副本、可选限额与两端预算经过定向验证。

PS02 只验证任务数据的装配、编解码和接纳。将可选限额落实到 Isolate 参数、两类环境及文件交接属于 PS03；checker 执行、严格结果 JSON、CheckerFailed 归因与资源展示属于 PS04。CV05、CV06、CV08 在本阶段仅记录已覆盖的装配与协议部分，完整执行及真实链路由后续任务验收。

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

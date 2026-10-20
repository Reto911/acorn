# MySQL 迁移实施记录

对应设计：[09 MySQL 迁移设计与执行方案](../09-mysql-migration-design.md)；已确认决策和审核过程见[审核记录](../d01-review/09-mysql-migration-review.md)。

## 1. 当前状态

M01–M04 已完成：MySQL 结构、连接、SQLx 0.9.0 检查式解码、幂等竞争读取、审查行锁、故障注入及相关 Rust 检查已完成，M03／M04 的实际范围见第 8／9 节。当前共 15 个工作项已完成，M06、M07 的五个工作项待完成。正式部署配置未修改，应用库未初始化。具体任务定义和完成条件以设计第 10 节为准；原 M05、M08 已按审核结论取消。

### 1.1 任务完成状态

设计第 10 节保留 20 个工作项的定义，完成情况在本表维护。

| 任务 | 已完成工作项 | 状态 |
| --- | --- | --- |
| M01 | 3／3 | 完成 |
| M02 | 4／4 | 完成 |
| M03 | 4／4 | 完成 |
| M04 | 4／4 | 完成 |
| M06 | 0／2 | 未开始 |
| M07 | 0／3 | 未开始 |

## 2. 文档工作

R01 已按审核反馈修订方案为 v2：明确全新部署和版本 4，改为 SQL 直接转换及任务待办清单，增加运行期事务说明和原实现对照；建立独立审核／实施记录，并更新索引与长期记录位置规则。

R02 将 D04 和密码供给方式记为已确认，更新任务依赖和索引；补充 READ COMMITTED 与默认 REPEATABLE READ 的区别、选择依据及适用范围，明确接纳短事务、无数据库资源的模型等待、完成短事务三个阶段。既有 20 项迁移待办仍全部未开始。

R03 将 READ COMMITTED 记为已确认，核对全文后明确当前无待决设计事项，并同步方案、审核记录及索引。迁移代码实施和数据库操作仍未开始，任务范围与待办状态不变。

R04 按用户批准将方案状态更新为已批准，设计、审核／实施记录、文档索引及既定记录目录规则已提交，提交说明为 `docs: record approved MySQL migration plan`。迁移代码和数据库操作均未开始，原有 20 项待办保持未勾选。

上述工作仅修改文档和 AGENTS.md 的长期工作规则，未修改 Rust、SQL、Proto 或实际运行配置，未读取开发凭据实际值。

## 3. 检查结果

本节保留 R01–R03 审核文档的原检查范围；M01 实际检查另见第 4 节，M02 见第 5 节。

文档格式、保留后的章节／任务编号、相对链接／锚点、代码块、表格和 TOML／shell 示例语法检查通过；20 项待办及已删除范围的检查通过，`git diff --check` 通过。Rust、前端、MySQL 和真实 Worker 验收均未执行。

R02 修订后再次检查上述文档项目，结果通过；新增资源边界表格和既有审查合同链接有效，D04 与密码方式不再残留待审状态，章节／任务编号及 20 项未勾选待办保持不变。隔离等级默认值、快照和锁规则按 MySQL 8.4 官方文档核对；未将文档核对记作运行验证。

R03 的格式、链接／锚点、表格、代码块及 TOML／shell 示例语法检查通过，章节／任务编号与 20 项未勾选待办保持不变；READ COMMITTED 的待审表述已清除，`git diff --check` 通过。本轮仍仅作文档检查。

索引中既有 sql/README.md 缺失保留为 M07 的说明补齐任务，不计为新增链接检查通过的范围。

后续实施按任务记录实际交付、执行命令、结果及对应 V01–V05 的覆盖范围；环境或权限限制、失败检查和剩余工作如实保留，不记录本机路径或临时产物清单。

## 4. M01：MySQL 结构和预置 SQL

### 4.1 已实现内容

- 初始 SQL 改为 MySQL 8.4／InnoDB，使用 utf8mb4_0900_bin、严格模式、有符号 BIGINT、UUID VARCHAR、LONGBLOB 和 LONGTEXT。保留六条 RESTRICT 外键、枚举、正值和终态／审查关系检查，JSON_TYPE 由 CASE 与 JSON_VALID 保护，并将 CASE 结果显式转为布尔条件。
- 发布指针外键在业务表创建后通过 ALTER TABLE 加入，排队索引改为 `(state, created_at_ms, id)`；结构版本使用固定主键表，并在全部 DDL 成功后写入 `(1, 4)`。
- 开发用户和示例题改为 MySQL DML 事务与 Unix 毫秒时间；现有 208 个字节表达式自动转换为十六进制二进制字面量，不恢复历史生成工具，保留用户身份、密码哈希、题面、语言限额及 104 个用例。
- 同步第 02 篇的结构合同、第 09 篇初始化说明和文档索引，明确 MySQL SQL 与尚未切换的 SQLite 后端之间的阶段边界。

### 4.2 实际检查与修正

静态内容比对通过：开发用户身份及密码哈希、标题与题面、C/Python 限额和全部 104 个用例逐字节及顺序一致。结构检查确认七张 InnoDB 表、六条外键、版本标记位于全部 DDL 之后，三个脚本均显式设置字符集和严格模式，已移除 SQLite 方言。

四份变更文档的格式、61 处有效本地链接／锚点、代码块、shell／TOML 示例语法及保留的 20 项待办编号检查通过；既有 sql/README.md 缺失仍单独列为 M07 待办，不计为链接检查通过。三个 SQL 的布局及 DDL／DML 事务结构检查、`git diff --check` 通过。

首次连接时，工具沙箱禁止建立网络连接；使用获准的宿主网络后确认目标为 MySQL 8.4.11。此前创建独立库返回 1044（开发账户权限不足），未执行初始化。用户随后创建并授权专用验证库，验证入口改为使用该空库，不要求新增全局建库权限。

首次实际执行初始结构时返回 3812：MySQL 不接受 JSON CHECK 中直接返回整数的 CASE 表达式。已将两处 CASE 结果显式比较为 `= 1`，保持 NULL、有效对象和无效 JSON 的原有判断语义，不删除或放宽约束。本轮测试表清理后，从空库重新执行全部验证通过。

使用临时 PyMySQL 1.1.2 客户端，通过仓库外验证入口依次执行三个正式 SQL 脚本及 V01 的 SQL 层检查，结果如下：

| 检查 | 实际结果 |
| --- | --- |
| 初始化及结构 | 三个脚本成功执行；七张表均为 InnoDB／utf8mb4_0900_bin；结构版本为 `(1, 4)`；字段类型、有符号整数、六条完整 RESTRICT 外键、查询索引及 21 项已启用 CHECK 符合设计 |
| 预置内容 | 用户身份及密码哈希、题目标题和题面、发布指针、两种语言限额、全部 104 个用例及其顺序和原始字节均与转换前一致；时间戳保存为正值 Unix 毫秒整数 |
| 文本和正值边界 | 大小写、重音及末尾空格不同的用户名可分别保存，完全相同的用户名被唯一键拒绝；255 个中文字符可保存，256 个被严格模式拒绝；中文及四字节字符往返、正值版本／限额／用例序号检查通过 |
| 外键及唯一键 | 幂等键重复被拒绝；不存在的用户、版本语言以及跨题目发布指针被拒绝；被引用用户和题目版本的删除、被引用版本号更新被 RESTRICT 拒绝，正值 CHECK 与 RESTRICT 可以共存 |
| 容量及原始字节 | 源码、输入和期望输出各 65,536 字节逐字节往返，包含空字节、换行及非 UTF-8 字节；按既有 CE 详情字段保存的 87,453 字节 Base64 JSON 文本原样往返 |
| 状态及 JSON | queued 默认空值关系、合法 AC／CE 和审查字段保存通过；非空源码、精确状态枚举、终态／审查空值关系拒绝非法组合；两类详情均拒绝数组和格式错误的 JSON，受保护的 JSON_TYPE 检查生效 |
| 收尾 | 验证事务回滚，清理本次创建的全部测试表，确认专用验证库为空并关闭连接；预建数据库保留，应用库未初始化 |

上述检查完成 V01 的结构、预置及 SQL 存储容量部分，不代表 SQLx 解码、后端并发、生命周期或真实判题通过。

### 4.3 剩余工作与验证边界

本步未修改 Rust、Cargo、Proto、前端或正式运行配置，未执行 Rust 编译／Clippy、后端集成、前端和真实 Worker 验收；这些检查按 M02–M04、M06 的依赖继续执行。

M01 交付时的后续安排为 M02 切换连接、配置和访问层，并建立 MySQL 测试夹具；V01 的后端类型往返及 V02–V04 继续由 M03／M04 检查，V05 留在 M06。SQL README 仍由 M07 补齐，M01 当时的 SQLite 后端和测试不能直接执行已转换的 MySQL 脚本。

## 5. M02：连接、配置和访问层

本节保留 SQLx 0.8.6 初次交付时的实现和检查；随后采用上游修复的过程与结果见第 7 节。

### 5.1 已实现内容

- SQLx 保持 0.8.6，驱动特性改为 mysql 并由 Cargo 更新锁文件；后端运行依赖树已移除 SQLite 驱动和 libsqlite3。配置改为端点、库名、用户及连接预算，删除旧 path／busy_timeout_ms 并拒绝未知字段，连接数默认 4。
- 配置解析不读取秘密；共同连接入口从进程 MYSQL_PASSWORD 读取密码，缺少或不是有效 UTF-8 时返回不包含原值的启动错误。选项通过字段构造，禁用 MySQL SSL；配置对象 Debug 和应用日志不输出数据库密码，子进程 TOML 只含非秘密连接字段，正式程序和测试不自动读取 .env。
- 应用共用有界 MySqlPool，每条新连接设置目标字符集／排序规则、READ COMMITTED、严格模式、InnoDB 和元数据锁等待；取得连接预算由 SQLx 池覆盖。启动读取版本 4 和预置用户，校验失败关闭池，正常停机关闭全部池连接。
- 数据库四个模块改为 MySQL 行、连接和短事务起止，保留参数绑定、固定版本及字节／JSON 类型。严重存储分类初步改用 MySQL 数值错误号，普通网络 I/O 不按磁盘损坏处理；完整故障行为留待 M03／M04 验收。
- database 与 workflow 共用一个 MySQL 夹具，专用测试 TOML 通过 ACORN_TEST_DATABASE_CONFIG 指定；name 为 1–32 位 ASCII 字母数字前缀，每例追加 UUID，依次执行三个正式 SQL。子进程使用对应的非秘密配置和进程密码；正常收尾先结束服务、关闭连接池，再删除本例库，临时目录只保存进程配置等文件。
- 同步第 02／03 篇连接和启动合同，更新第 07／08 篇受影响的共享池说明，以及 backend、根中英文 README 的初始化、配置、密码和检查用法。M02 计划中的原四项待办保持编号，均已满足完成条件，状态见第 1.1 节。

### 5.2 兼容性问题及修正

SQLx 0.8.6 的握手排序规则枚举不包含 utf8mb4_0900_bin。按锁定版本源码核对后，握手使用驱动支持的 utf8mb4_bin，连接初始化通过 SET NAMES 切到目标规则，再供应用使用；未改变表或会话最终比较语义。

首次 SQLx 基础读写检查实际读取 title 时返回 String 与 BLOB 不兼容：二进制排序规则使文本列携带 BINARY 标志，SQLx 的字符串类型兼容性检查拒绝该标志。已对明确的 VARCHAR／LONGTEXT 投影使用 try_get_unchecked 跳过这项类型兼容性检查，UTF-8 解码和可空校验仍保留，原始字节列继续按 Vec<u8> 读取。清理测试表后重新执行基础检查通过。

连接初始化曾因 raw_sql future 的生命周期约束不能用于池的 after_connect 回调而编译失败，改为逐项参数化查询后编译通过。首次依赖获取受工具沙箱 DNS 限制，获准使用宿主网络后完成；未增加数据库连接失败的运行期回退或自动重试。

独立测试库权限到位后的首次夹具检查返回 1295：MySQL 不支持通过预处理协议执行 USE；随后启动检查中的 START TRANSACTION 也返回同一错误。已将这两处及对应 ROLLBACK 改为 raw_sql 的文本协议执行，保留参数化业务查询；明确清理本次失败夹具库后，相应检查通过。

### 5.3 实际检查

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- `cargo test -p acorn-backend --locked --lib --test config_results`：10 项单元测试及 7 项配置／结果测试通过。新增配置边界、旧字段拒绝、默认连接数及缺少 MYSQL_PASSWORD 的真实子进程启动检查通过；原 SQLite 损坏文件单测改为普通网络错误分类检查，MySQL 严重服务端错误的通知行为仍待 M03／M04 验收。
- 使用已授权的独立验证库运行临时开发入口，调用正式 Database／connect_pool，不启动 Worker 或模型：两个实际应用连接均取得 utf8mb4_0900_bin、READ COMMITTED、严格模式及两类锁等待；池上限耗尽返回 PoolTimedOut，归还连接后可再次取得连接。题目与固定版本读取、104 用例装配、顺序同键复用、历史与 NULL 字段、65,536 字节源码／原始测试数据、超过 64 KiB 的 CE JSON 存取及正式类型解码通过。每轮清理测试表并确认验证库为空，临时 Rust 检查入口已移除。
- 正式 database 检查使用 `cargo test -p acorn-backend --locked --test database -- --test-threads=1 --skip concurrent_idempotency_and_conflict`：USE 修正后 8 项通过，启动锁检查因上述 1295 失败；修正事务控制后，以 `cargo test -p acorn-backend --locked --test database startup_checks_and_finite_lock_wait -- --test-threads=1` 重跑该项通过。共 9 项完成基础验收，覆盖独立库正式初始化、双连接会话与池预算、固定版本／历史、终态／审查类型存取、外键、缺库不创建、版本行／版本号／用户启动拒绝，以及实际一秒行锁等待。CE 终态存储测试已通过 65,536 字节诊断和超过 64 KiB JSON 的正式类型解码。并发同键检查留待 M03。
- 正式 workflow 使用 `cargo test -p acorn-backend --locked --test workflow published_queries_and_history_shapes -- --test-threads=1` 与 `cargo test -p acorn-backend --locked --test workflow process_startup_loads_queue_and_interrupts_in_flight -- --test-threads=1`：两项通过。共享夹具、HTTP 查询与历史、受控 gRPC Worker、子进程非秘密配置与进程密码、既有队列加载和正常 SIGTERM 收尾可用；进程测试未调用真实 Worker 或模型。各例关闭服务及池后删除其独立库，收尾只读确认无本批测试库残留。
- `cargo tree --locked -p acorn-backend --edges normal`：仅包含 SQLx MySQL 驱动，不再包含 SQLite 运行依赖。
- 10 份变更文档的格式、215 处有效本地链接／锚点、表格、代码块、shell／TOML 示例及中英文连接字段同步检查通过；三个原有 sql/README.md 引用仍按 M07 单独保留，不计为链接通过。任务总数仍为 20，其中七项已完成、13 项待完成，`git diff --check` 通过。

### 5.4 当前限制及剩余工作

MySQL 8.4.11 此前创建独立测试库返回 1044；用户补充限定 acorntest 前缀的开发授权后，本次创建／删除检查及正式夹具、子进程基础检查通过，权限阻碍已解除，M02 完成。

M03 继续实现并验证 1062 竞争读取、审查行锁、条件完成及故障边界；M04 继续转换尚存的 SQLite 专用锁注入／拒写触发器 SQL 和相应断言。本次基础检查不覆盖这些并发和故障合同，不将其记为 V02–V04 整体验收通过。

本批没有执行全量 workspace 测试、真实 Isolate／Worker、GUI 或真实模型调用。V02–V05 保持原后续任务范围；SQL README 缺失仍留在 M07。

### 5.5 初始实施安排

以下为原设计 10.2.1 中记录的 M02 实施细化，归入本记录；设计第 10.2 节保留任务职责、顺序和验收边界。

| 顺序 | 工作与位置 | 检查要求 |
| --- | --- | --- |
| 1 | 修改后端 config.rs、backend.example.toml 和 config_results：按第 3.1 节加入端点、库名、用户名和连接预算，默认 max_connections=4；校验 host:port／带方括号的 IPv6、非空名称及正值和有效范围，删除 path／busy_timeout_ms | 新配置可解析，旧字段被未知字段规则拒绝；配置解析与校验保持不读取秘密，正式连接入口读取 MYSQL_PASSWORD，缺少时启动明确失败，Debug 和日志不输出密码或完整连接选项 |
| 2 | 将 workspace SQLx 0.8.6 驱动特性改为 mysql，更新锁文件；在 database/mod.rs 建立唯一的 MySQL 应用连接池入口，使用字段接口构造连接选项并禁用 SSL | 每条新连接设置字符集、排序规则、READ COMMITTED、严格模式和两类锁等待；池上限与 acquire 预算生效，启动检查 schema_version 的版本 4 及配置用户，检查失败关闭池 |
| 3 | 转换 database/mod.rs、problems.rs、submissions.rs、reviews.rs 的池、连接和行类型，适配事务起止及 SQL 方言，核对参数和解码 | 基础读写经过共同连接入口；有符号整数、EXISTS／可空结果、UUID 文本、原始字节及 JSON 文本按既有类型往返，固定版本任务装配保持；全部受影响目标可编译 |
| 4 | 适配 tests/database.rs 及 tests/support 下的数据库与子进程夹具，共用实际重复的建库、初始化和关闭流程；增加无秘密的专用测试 TOML 示例，接入 ACORN_TEST_DATABASE_CONFIG | 每例创建独立库并执行三个正式 SQL，应用句柄使用该库；独立控制连接供后续锁测试使用；子进程得到对应非秘密配置及 MYSQL_PASSWORD；停止服务、关闭连接和池后删除本例库，临时目录仅承担进程配置等文件职责 |
| 5 | 同步第 02／03 篇连接和启动合同、backend README、根中英文 README，以及受影响的配置说明 | 初始化、密码供给、启动和测试命令与已实现入口一致，中英文功能内容同步；文档区分基础连接可用与 M03／M04 尚待验证的合同 |
| 6 | 补充配置、启动和 SQLx 基础读写的必要断言，执行本批 Rust 必需检查及相关测试 | 格式、workspace all-targets 编译、必需 Clippy、配置检查和受影响的基础读写检查有实际结果；新连接设置在至少两个应用连接下核对，未执行或失败的检查明确保留 |

原 10.2.2 的 MySQL 8.4.11 连通、前缀授权、独立库初始化和删除情况，已归入第 4.2、5.2–5.4 节；测试环境的一般要求保留在设计第 11.2 节。

## 6. 文档职责整理

按用户要求，迁移设计移除执行状态、实际验证结果和环境准备过程，删除 10.2.1／10.2.2 的混合记录，保留设计理由、模块职责、实施顺序和验收要求。原工作项和任务编号保持，完成状态改由本记录第 1.1 节维护；本轮新增到第 02／03 篇的迁移进度改为实施记录入口。审核讨论继续由对应审核记录维护，未搬动既有历史设计中的旧记录。

整理后的 12 份变更 Markdown 文档、225 处有效本地链接／锚点、表格、代码块、32 段 shell 和六段 TOML 示例检查通过；中英文配置字段和命令保持同步，20 个工作项与任务编号保留。三个既有 SQL README 缺失引用仍由 M07 处理。`git diff --check` 通过；本次整理仅改文档，沿用第 5.3 节已完成的代码检查。

文档整理与当前 MySQL 连接／夹具实现先提交为 `169c3b3`（`feat(db): connect backend and test fixtures to MySQL`），随后执行第 7 节的 SQLx 修正。事务并发和故障注入仍按 M03／M04 推进。

## 7. SQLx 文本类型识别修正

### 7.1 修正内容与依据

- SQLx 及相关依赖锁定为 0.9.0，采用[上游 #3924](https://github.com/transact-rs/sqlx/pull/3924) 对文本排序规则和 binary 数据的区分。原 VARCHAR／LONGTEXT 的 try_get_unchecked 全部恢复为 try_get，保留 SQL 类型匹配、UTF-8、NULL、JSON 和枚举校验；原始字节列继续按 Vec<u8> 读取。
- 连接选项直接配置 utf8mb4_0900_bin，由驱动在连接返回前执行字符集／排序规则设置；移除旧版 utf8mb4_bin 握手兼容处理和应用 after_connect 内重复的 SET NAMES。隔离等级、严格模式、池和两类锁等待预算保持，MySQL SSL 仍禁用。启用 mysql-rsa 保留无 TLS 时需要 RSA 的密码认证能力。
- 按 0.9 的 SqlSafeStr 接口适配既有格式化 SQL。应用内 AssertSqlSafe 只包装静态列清单与静态模板；测试夹具只插入经过既有 ASCII 字母数字前缀校验及 UUID 生成的本例库名。请求数据继续全部绑定参数，字符串解码不再跳过类型匹配。
- 在既有 database 测试中增加 mysql_binary_collation_preserves_text_type_checks：实际 VARCHAR／LONGTEXT 可读为 String，NULL 保持 Option 行为，包含合法 UTF-8 的真实 LONGBLOB 仍拒绝读为 String，并能按 Vec<u8> 原样读取。使用同一正式初始化夹具，无新依赖或自定义驱动补丁。
- 同步第 03／09 篇当前依赖和连接合同，中英文 README 同步注明 SQLx 0.9.0 要求的 Rust/Cargo 最低版本 1.94.0。第 5 节保留旧版本检查，不将历史兼容处理写成当前实现。

### 7.2 实际检查

| 命令／范围 | 结果 |
| --- | --- |
| cargo fmt --all -- --check | 通过 |
| cargo check --workspace --all-targets --locked | 通过 |
| cargo clippy --workspace --all-targets --locked -- -D warnings | 通过 |
| cargo test -p acorn-backend --locked --lib --test config_results | 10 项单元测试及 7 项配置／结果测试通过 |
| cargo test -p acorn-backend --locked --test database -- --test-threads=1 --skip concurrent_idempotency_and_conflict | 10 项通过，包含新增类型匹配回归、双连接会话、连接池预算、终态／审查 JSON、版本和启动检查；并发同键项未执行 |
| cargo test -p acorn-backend --locked --test workflow published_queries_and_history_shapes -- --test-threads=1 | 一项通过，覆盖共享夹具、受控 Worker 和 HTTP 题目／历史查询 |
| cargo test -p acorn-backend --locked --test workflow process_startup_loads_queue_and_interrupts_in_flight -- --test-threads=1 | 一项通过，覆盖子进程配置、既有队列及正常 SIGTERM 收尾 |
| cargo tree --locked -p acorn-backend --edges normal | SQLx／core／mysql 均为 0.9.0，保留 RSA 认证依赖，不含 SQLite 运行依赖 |

共 29 个测试用例通过。实际 MySQL 仍为 8.4.11；每例独立建库并执行正式 SQL，连接、池及进程收尾后删除本例库，最终只读确认本批测试库无残留。Rust 源码及测试中没有 try_get_unchecked，未保留旧握手排序规则回退。

文档整理及修正涉及的 12 份 Markdown、225 处有效本地链接／锚点、表格、代码块、shell／TOML 示例及中英文配置／命令同步检查通过。20 个设计工作项保持，完成状态仅在实施记录维护；三个既有 SQL README 缺失引用继续留在 M07。`git diff --check` 通过。

### 7.3 验证边界

本次验证覆盖文本类型识别修正及既有基础 MySQL 链路，没有执行全量 workspace 测试、并发同键及审查行锁验收、SQLite 专用故障注入的 MySQL 替换、真实 Isolate／Worker、GUI 或模型调用。M03／M04、M06、M07 的职责和剩余工作不变。

## 8. M03：事务和故障边界

### 8.1 已实现内容

- 创建提交只对本次 INSERT 返回的 MySQL 数值错误 1062 执行竞争读取：先显式回滚，再开启新短事务按同一用户和幂等键读取一次。初次查到与竞争后查到的记录共用内容比较／固定版本任务装配流程，同内容返回原 ID，不同内容返回 IdempotencyConflict；仅 queued 返回可交接任务。内容冲突先结束只读事务，回滚失败保留为存储错误。
- 同一键不存在时，结束读取事务并保留原 1062；其他 SQL 错误以及回滚、读取或提交错误继续交给关键写入故障观察。该分支不再次 INSERT，不把其他唯一键冲突转为幂等成功。
- 手动审查在原 READ COMMITTED 短事务中按提交主键 SELECT FOR UPDATE，保持不存在、非 finished、复用 PENDING、预期 ID、容量、新接纳的原判断顺序。事务结束并归还连接后才返回和交接，模型等待不持有连接或行锁；审查完成继续读取不可变源码并按 id、review_id、PENDING 条件写入，迟到结果 0 行是正常结果。
- 提取实际复用的 MySQL 数值错误号读取，供 1062 分支和既有严重存储分类共同使用。普通网络 I/O 不视为服务端存储损坏，关键写入 SQL 错误仍保留首个故障通知。沿用机器终态／自动审查初态的一次原子保存、保存成功后 ACK、创建与审查独立短操作，以及提交结果不明时不自动重做写入的边界。
- 同步第 02／03／07／08 篇当前事务、读取错误和审查存储合同；设计保持任务和理由，实际过程与完成状态仅在本记录维护。创建与审查服务的数据库句柄说明改为共享有界池。

### 8.2 实际并发和新增覆盖

复用并加强 concurrent_idempotency_and_conflict：控制连接锁定用户行，使用 SHOW FULL PROCESSLIST 确认本例库有四条应用连接进入 INSERT 后释放锁。八个同键请求包含两份源码，结果为一次新建、三次复用、四次内容冲突；只保存一条提交，未通知致命故障。该安排保证并发已经进入 INSERT，而不是只依赖异步请求碰巧重叠。会话观测只筛选本例库并不打印查询内容；账户可查看自身会话的依据见 [MySQL SHOW PROCESSLIST](https://dev.mysql.com/doc/refman/8.4/en/show-processlist.html)。

复用并加强 concurrent_first_manual_requests_share_one_attempt：控制连接先锁定提交行，确认四条应用连接进入接纳查询，再释放锁。八个并发 HTTP 请求只接纳一个 PENDING、返回同一审查 ID，受控 Responses 端只收到一次 POST。其他既有用例覆盖 PENDING 复用优先于容量／旧 ID、重跑、迟到完成、ACK 失败及停机。

本轮仅新增一个测试用例 unrelated_unique_conflict_remains_fatal：在本例测试库增加源码唯一索引，以不同幂等键触发真实 1062，确认找不到对应幂等记录时保留原错误、通知监督且不新增提交。既有 startup_checks_and_finite_lock_wait 增强为核对实际 1205 和关键写入故障通知；其余用例复用既有测试。

首次数据库及流程功能用例均通过，但收尾的测试库余量检查失败，发现一个库残留。定位为 database_failure_ends_tracking_without_ack 使用的 close_failed 夹具遗漏删除流程；补上共用 Fixture::close，明确清理该单例遗留库后，只重跑受影响用例通过。最终只读核对本批测试库残留为零。

### 8.3 实际检查

| 命令／范围 | 结果 |
| --- | --- |
| cargo fmt --all -- --check | 通过 |
| cargo check --workspace --all-targets --locked | 通过 |
| cargo clippy --workspace --all-targets --locked -- -D warnings | 通过；夹具收尾修正后再次执行通过 |
| 后端单元测试与 config_results | 10 项单元测试、7 项配置／结果测试通过 |
| database | 全部 12 项通过，包含同键竞争及新增的无关唯一键冲突用例 |
| workflow 的本步相关用例 | 17 项通过，其余 24 项未执行；故障退出用例修正收尾后单独重跑通过，不重复计数 |

本步共运行并通过 46 个不同测试用例。数据库和流程检查使用 MySQL 8.4.11、每例独立库及正式初始化 SQL，测试配置与进程密码由已授权开发入口供给，正式测试不加载 .env。实际测试命令如下：

```sh
cargo test -p acorn-backend --locked --lib --test config_results
cargo test -p acorn-backend --locked --test database -- --test-threads=1
cargo test -p acorn-backend --locked --test workflow -- --test-threads=1 \
  concurrent_creation_merges_and_rollback_releases_permits \
  queue_full_and_idempotency_preserve_identity_and_version \
  queued_rehandoff_keeps_original_version \
  terminal_replay_bypasses_capacity_and_late_result_is_acked \
  database_failure_ends_tracking_without_ack \
  concurrent_first_manual_requests_share_one_attempt \
  manual_http_admission_pending_merge_and_queue_full \
  manual_http_rejections_preserve_records \
  manual_rerun_expected_identity_and_preservation \
  manual_rerun_survives_previous_attempt_cleanup \
  automatic_fifo_capacity_and_worker_release \
  stop_between_terminal_commit_and_ack_completion \
  stale_machine_terminal_releases_review_capacity \
  unavailable_review_and_ack_failure_keep_machine_results \
  process_review_startup_interrupts_without_reposting \
  process_review_shutdown_cancels_and_marks_new_terminals \
  process_shutdown_drains_running_and_restart_loads_queued
cargo test -p acorn-backend --locked --test workflow \
  database_failure_ends_tracking_without_ack -- --test-threads=1
```

六份变更文档的格式、144 处有效本地链接／锚点、表格、代码块、三段 shell 和两段 TOML 示例检查通过；唯一涉及的既有 SQL README 缺失引用仍留在 M07。20 个设计工作项及原章节编号保持，当前 11 项完成、九项待完成的状态仅在实施记录维护。`git diff --check` 通过。

### 8.4 完成状态与剩余边界

M03 四个工作项完成。V02 的并发创建／手动接纳，以及 V03／V04 中相应的条件保存、迟到结果、ACK 和既有重启／停机路径已作本节范围内检查；不据此宣称 V02–V04 整体验收完成。

M04 继续转换尚存的 SQLite 锁注入和拒写触发器，并执行相应普通读取失败恢复、关键拒写和断开 HTTP 等用例。本轮未做回滚故障、COMMIT 响应丢失或服务端存储损坏注入，也未执行完整 workspace、真实 Isolate／Worker、GUI 或真实模型调用；不新增这些故障的组合矩阵要求。M06／M07 依赖与范围不变。

## 9. M04：测试适配与 Rust 检查

### 9.1 已实现内容

- 适配八个既有 workflow 用例中的 SQLite 锁语句和拒写触发器，没有新增测试框架或用例。健康读取使用独立控制连接的 `LOCK TABLES users WRITE`，题目查询和创建前幂等查询使用对应表锁；控制连接先关闭自动提交，结束时显式 COMMIT 和 UNLOCK TABLES。普通 SELECT 不依赖行写锁阻塞。
- 健康用例将数据库锁等待设为一秒，读取失败分支的探测预算设为三秒，超时分支保留十毫秒；实际检查分别取得 DatabaseReadFailed 和 ProbeTimeout、失败缓存及 HTTP 展示，并在解锁后恢复，不触发后端故障或判题 RPC。
- 创建写入通过锁定用户行阻塞 INSERT 的外键检查，手动接纳通过锁定目标提交行阻塞 SELECT FOR UPDATE。断开 HTTP 的创建／审查用例和停机等待审查用例复用既有查询观测入口，确认应用已到达相关 SQL 后再取消 HTTP 等待；解锁后仍完成已登记短操作及相应交接或中断收尾。创建写入超时明确核对 1205、故障退出、记录未新建及未 ACK。
- 终态和审查拒写触发器改为 MySQL FOR EACH ROW、条件 IF 和 SIGNAL SQLSTATE '45000'，保留受控错误说明、机器结果／PENDING 保存状态、ACK 数量及故障退出断言。模型 API 失败仍只结束该次审查。
- 复用既有服务停止、连接及池关闭、独立库删除流程，并修正测试组装的数据库说明。迁移设计补充启用 binlog 时创建触发器的环境要求；应用和测试不修改 MySQL 全局变量。

### 9.2 实际失败与环境准备

首次网络检查在工具沙箱中返回 Operation not permitted；切换到获准的宿主网络后，确认 MySQL 为 8.4.11，独立测试库创建和删除通过。

首次八项定向 workflow 检查中，六项通过，两个拒写用例在创建触发器时返回 1419：实例已启用 binlog，且 log_bin_trust_function_creators 为 0。该失败发生在故障注入准备阶段，没有进入预期的拒写断言。先完成其余检查，按创建时间和受控记录确认并明确删除这两个失败用例留下的独立库，没有增加清库扫描或自动修复流程。

用户随后由管理员将开发实例的 log_bin_trust_function_creators 设为 1。只重跑受影响的两个触发器用例，均通过；没有将首次失败改写为通过。最终只读确认独立测试库残留为零，正式应用库未初始化。

### 9.3 实际检查

| 命令／范围 | 结果 |
| --- | --- |
| cargo fmt --all -- --check | 通过 |
| cargo check --workspace --all-targets --locked | 通过 |
| cargo clippy --workspace --all-targets --locked -- -D warnings | 通过 |
| cargo test -p acorn-backend --locked --lib --test config_results | 10 项单元测试、7 项配置／结果测试通过 |
| database | 全部 12 项通过 |
| workflow | 全部 39 项常规测试通过：先通过不依赖触发器的 37 项，环境准备后通过其余两项；两个真实 Worker 验收按原规则忽略 |

共运行并通过 68 个不同常规测试用例，定向检查和重跑不重复计数。数据库及流程检查由已授权开发入口供给测试 TOML 和进程 MYSQL_PASSWORD，正式测试不自动加载 .env；每例使用独立 MySQL 库及三个正式初始化 SQL。实际测试命令如下：

```sh
cargo test -p acorn-backend --locked --lib --test config_results
cargo test -p acorn-backend --locked --test workflow -- --test-threads=1 \
  health_database_read_failure_and_timeout_recover \
  disconnected_create_finishes_transaction_and_handoff \
  process_read_failures_keep_serving \
  process_creation_failure_exits_without_draining_workers \
  process_fault_during_shutdown_does_not_wait_other_worker \
  disconnected_manual_http_finishes_admission \
  process_shutdown_drains_registered_manual_admission \
  review_api_failure_and_critical_completion_failure
cargo test -p acorn-backend --locked --test database --test workflow -- --test-threads=1 \
  --skip process_fault_during_shutdown_does_not_wait_other_worker \
  --skip review_api_failure_and_critical_completion_failure
cargo test -p acorn-backend --locked --test workflow -- --test-threads=1 \
  process_fault_during_shutdown_does_not_wait_other_worker \
  review_api_failure_and_critical_completion_failure
```

两份变更文档的格式、23 处本地链接／锚点、表格、代码块、四段 shell 和一段 TOML 示例检查通过；原章节编号及 20 个设计工作项保持，中英文根说明的既有配置字段和命令一致。`git diff --check` 通过。

### 9.4 完成状态与剩余边界

M04 四个工作项完成。V01–V04 中约定的结构／类型、幂等和固定版本、条件终态和审查、ACK、断开 HTTP、重启／停机、普通读取失败恢复及关键拒写路径已按本节和前序记录的有限范围验证，不扩展为全部故障与状态组合。

未执行完整 workspace 测试、真实 Worker／Isolate、GUI 或真实模型调用，也未注入回滚失败、COMMIT 响应丢失、真实网络断连或服务端存储损坏。M06 继续完成真实判题和受控审查展示的 V05 验收；M07 补齐 SQL README 并核对受影响文档、配置、命令及链接。

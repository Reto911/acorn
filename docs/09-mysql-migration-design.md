# MySQL 迁移设计与执行方案

状态：v2，已获用户批准，M01 已完成；结构和预置 SQL 已转换并通过独立空库的 SQL 验证。D01–D04、密码供给方式及 READ COMMITTED 已确认，当前无待决设计事项。当前后端程序仍使用 SQLite 版本 3，M02–M04 接通后仅使用 MySQL，通过新库初始化完成整体重新部署。[审核记录](d01-review/09-mysql-migration-review.md)与[实施记录](d02-execution/09-mysql-migration-execution.md)单独维护。

## 1. 目标与审核范围

将后端持久化改为 MySQL，保持现有判题、幂等提交、固定题目版本、最新一次 LLM 审查和健康观测的业务行为。复用 SQLx、现有数据库模块、服务生命周期及测试，Worker、Proto 和 HTTP DTO 保持现有合同。

| 标识 | 当前结论 | 状态与范围 |
| --- | --- | --- |
| D01 | 目标为 MySQL 8.4、InnoDB，完成后仅支持 MySQL | 已确认；不保留 SQLite 运行支持或离线读取入口，实施中可参考原代码 |
| D02 | 整体重新部署，数据库从空库初始化；继续沿用单后端实例架构 | 全新部署已确认；单后端是现有架构，一个后端仍可连接多个 Worker，与在线数据库升级无关 |
| D03 | 不保留旧数据，直接改写已有 SQL 并重新初始化 | 已确认；无旧数据库导入、升级、切换或回退任务 |
| D04 | 字段边界和只读网络错误的处理方式 | 已确认第 4.3 节的三项差异；故障处理见第 6 节 |

结构版本从 4 开始，当前不考虑 SSL。已确认的连接密码供给方式见第 3.1 节。具体任务使用第 10 节的待办清单跟踪；本次没有增加账户功能、题目管理 API、自动数据库升级或通用恢复框架。

## 2. 当前实现与改动位置

当前后端仍使用 SQLite 结构版本 3；M01 已将初始 SQL 转为 MySQL 结构版本 4，包含 `users`、`problems`、`problem_versions`、`problem_version_languages`、`test_cases`、`submissions` 六张业务表及 schema_version 表。迁移期间参考[数据库设计](02-database-design.md)、[后端设计](03-backend-design.md)和 [LLM 审查设计](08-llm-code-review-design.md)的业务合同；实现变更时同步相关现行设计。

| 位置 | 当前依赖 | 迁移内容 |
| --- | --- | --- |
| [workspace 依赖](../Cargo.toml) | SQLx 0.8.6 的 `sqlite` 特性 | 保留版本，正式驱动改为 `mysql`，移除 SQLite 特性并更新锁文件 |
| [数据库模块](../crates/backend/src/database/mod.rs)及其三个子模块 | 单连接 SqlitePool、SQLite 行／连接类型、PRAGMA、短写事务 | MySQL 连接池、类型解码、版本读取、事务和错误分类 |
| [后端配置](../crates/backend/src/config.rs)和[示例](../config/backend.example.toml) | 文件路径、busy_timeout_ms | 端点、库名、用户、密码供给、连接池和有限锁等待 |
| [初始结构](../sql/001-initial-schema.sql)、[示例题](../sql/002-example-problem.sql)、[开发用户](../examples/seed-user.sql) | 已转换为 MySQL DDL、时间函数及二进制字面量，独立库 SQL 验证通过 | 保留现有预置内容，初始化结构版本 4 |
| [数据库测试](../crates/backend/tests/database.rs)、[workflow](../crates/backend/tests/workflow.rs)及其夹具 | 临时文件库、SQLite 锁和触发器 | 独立 MySQL 测试库、对应事务及受控错误 |
| [程序入口](../crates/backend/src/main.rs)、[健康采集](../crates/backend/src/health.rs) | 共享句柄及现有失败通知 | 保留服务生命周期，接入新的连接和故障分类 |

索引引用的 `sql/README.md` 当前缺失，M07 补齐与实际实现一致的初始化和使用说明。旧阶段的 SQLite 验收仍按其原范围保留，不作为 MySQL 验收结果。

## 3. 连接、配置与启动

### 3.1 配置、密码与连接池

已确认使用以下后端配置，将端点、库名和用户名分开填写；密码由进程环境变量 `MYSQL_PASSWORD` 提供。此配置接口在 M02 实现前不能用于当前程序。

```toml
[database]
endpoint = "127.0.0.1:3306"
name = "appdb"
username = "example_user"
max_connections = 4
acquire_timeout_ms = 5_000
lock_wait_timeout_secs = 3
```

- `endpoint` 为 host:port，IPv6 使用 `[address]:port`；name 和 username 非空。使用 MySqlConnectOptions 的字段接口配置，密码不拼接进 URL。
- 正式后端读取 MYSQL_PASSWORD，缺少时在启动阶段明确报错；配置对象 Debug 和日志不输出密码，也不输出包含凭据的完整连接选项。
- `.env` 中的 MYSQL_ENDPOINT、MYSQL_DATABASE、MYSQL_USERNAME 分别对应上述非秘密配置，MYSQL_PASSWORD 对应进程密码变量。开发阶段可由临时入口按需读取用户提供的 .env，构造配置并向后端／测试子进程传入密码；正式程序不自动加载 .env，不新增 dotenv 依赖。正式部署由启动环境提供该变量。
- 当前显式使用 `MySqlSslMode::Disabled`，不增加 SSL 配置项或 TLS 接入步骤。现有 LLM 客户端的 TLS 不受影响。
- max_connections 为正整数，默认 4，限制 API、Scheduler、审查和健康采集共用池中的连接总数；并发合同须在至少两个应用连接下验证。
- acquire_timeout_ms 限制取得可用连接的整个过程：等待空闲连接、建立新连接及鉴权／初始化、SQLx 对复用连接的可用性检查都可能消耗这段时间。池不会先超出最大连接数再等待。
- 查询读取结果期间会占用连接；事务从开始到提交／回滚期间持续占用同一连接。操作完成后归还池，连接可保持空闲以供复用。Worker 执行和 LLM 网络等待不持有数据库连接。
- lock_wait_timeout_secs 同时设置会话 innodb_lock_wait_timeout 和元数据锁 lock_wait_timeout，单位为秒，取两项支持范围内的正值。它与连接取得等待是两种不同的等待，也不构成全部 SQL 的总期限。

删除旧 path 和 busy_timeout_ms，沿用拒绝未知配置字段的行为。应用账户只承担业务读写，不执行建库或建表；初始化与测试建库由对应开发／部署账户执行。

每条新连接初始化以下设置。它们限定当前连接，不修改整个 MySQL 服务的全局配置；初始化失败由驱动报告，不在每次查询前重复检查。

取得连接的等待范围以 [SQLx 0.8.6 连接池说明](https://docs.rs/crate/sqlx-core/0.8.6/source/src/pool/options.rs)为依据；查询执行和事务等待不计为 acquire 阶段。

| 设置 | 含义与用途 |
| --- | --- |
| utf8mb4 | 连接与文本使用完整的 MySQL UTF-8 字符集，支持中文及需要四字节表示的字符 |
| utf8mb4_0900_bin | 文本按二进制排序规则比较，区分大小写、重音及末尾空格，保持枚举和唯一键的精确比较 |
| READ COMMITTED | 事务中的每次普通读取看到该语句开始时已提交的数据；并发写入保护由唯一键、行锁和条件 UPDATE 承担 |
| STRICT_ALL_TABLES | 对不合法、越界等写入报错，避免仅警告后截断或调整数据 |
| NO_ENGINE_SUBSTITUTION | 明确要求的存储引擎不可用时拒绝 DDL，不自动改用其他引擎 |

SQL 初始化会话也明确设置所需字符集及严格模式。含义依据 [MySQL SQL 模式](https://dev.mysql.com/doc/refman/8.4/en/sql-mode.html)、[二进制排序规则](https://dev.mysql.com/doc/refman/8.4/en/charset-binary-collations.html)和[隔离级别](https://dev.mysql.com/doc/refman/8.4/en/innodb-transaction-isolation-levels.html)；连接使用现有 [SQLx 0.8.6 接口](https://docs.rs/sqlx/0.8.6/sqlx/mysql/struct.MySqlConnectOptions.html)。

READ COMMITTED（读已提交）是事务隔离等级；InnoDB 默认使用 REPEATABLE READ（可重复读），部署端可调整默认值。两者都不会普通读取其他事务尚未提交的数据。REPEATABLE READ 的普通一致性读取在同一事务内复用首次读取建立的快照；READ COMMITTED 则每条语句建立新的快照，同一事务内重复查询可能看到其他事务已提交的变化。`SELECT ... FOR UPDATE` 属于锁定读取，不能直接套用普通快照读取的规则。

本方案已确认采用 READ COMMITTED，依据是现有业务如何保证一致性：

- 创建提交先固定题目版本，随后按该版本读取不可变的题面、语言限额和用例，不需要靠事务快照维持多次读取的一致；幂等竞争由唯一键裁定。
- 手动审查在同一短事务内锁定提交行后判断并写入 PENDING；终态与审查完成由带状态／审查 ID 条件的 UPDATE 保护。
- 这些操作不要求重复读取同一范围得到相同集合；READ COMMITTED 的逐语句快照足以承载既有合同，并减少范围锁定读取／更新中的间隙锁。外键和唯一键检查仍可能使用间隙锁，死锁也仍需按第 6 节处理。

REPEATABLE READ 也可以实现上述业务合同。第 5.1 节的 1062 分支已经结束原事务，在新事务中读取，因此不依赖 READ COMMITTED 才能看见竞争方提交的记录。采用 READ COMMITTED 是与当前短事务和显式并发约束匹配的已确认设计，不代表已经验证性能更优。

### 3.2 结构版本与启动

新库结构版本从 **4** 开始，使用 schema_version 表保存单行 `(id=1, version=4)`，id 为固定主键。它表示本项目的数据结构，不表示从旧库执行过一次升级，也不代替 MySQL 服务端版本。

部署端执行完整初始化 SQL，版本行在全部 DDL 成功后写入。应用启动只读取版本并检查配置用户存在，不建表或升级，也不逐项扫描结构。缺少版本、版本不符或用户不存在均明确拒绝启动。

首次部署的新库没有旧任务。此后 MySQL 后端的正常重启仍沿用原有生命周期：中断上次运行遗留的机器在途记录和 PENDING 审查、装入 queued、启动服务。这些都是新系统自己的重启行为，与搬迁旧数据库无关。Scheduler 和审查活动仍要求只有一个活动后端。

## 4. 表结构与存储合同

### 4.1 字段映射

所有业务表使用 InnoDB，文本默认 `utf8mb4 COLLATE utf8mb4_0900_bin`。

| 字段 | MySQL 表示 | 语义 |
| --- | --- | --- |
| users.id、problems.id | 有符号 BIGINT AUTO_INCREMENT PRIMARY KEY | Rust、HTTP 继续使用正值 i64，显式预置 ID 必须为正 |
| 用户／题目引用、版本号、用例序号 | 有符号 BIGINT | 外键两端类型一致，非自增字段保留相应正值约束 |
| 时间戳和资源限额 | 有符号 BIGINT | 保留 UTC Unix 毫秒及现有 ms／KiB／bytes 单位，未知值保留 NULL |
| 提交 ID、幂等键、审查 ID | VARCHAR(36) | 保留规范 UUID 文本及既有可空关系 |
| username_normalized | VARCHAR(255)，完整唯一索引 | 最多 255 字符，精确唯一比较，见第 4.3 节 |
| 语言 | VARCHAR(16) | 保留 c、python 检查和联合外键 |
| 角色、用户状态、判题状态、verdict、审查模式与状态 | VARCHAR(32) | 保留枚举 CHECK；比较模式使用 VARCHAR(64) 并保留原检查 |
| 展示名称、密码哈希、标题、题面 | LONGTEXT | 保留文本，示例用户密码哈希和题面内容不改写 |
| 源码、测试输入、期望输出 | LONGBLOB | 保存原始字节，源码仍非空 |
| 机器终态和审查详情 | 可空 LONGTEXT，附 JSON 对象检查 | 复用 serde 类型和序列化字节预算，保留 JSON 文本往返 |

LONGBLOB 单列容量为 `2^32 - 1` 字节，约 4 GiB；足以覆盖当前默认单项 65,536 字节及单任务 8 MiB 合计预算。应用接纳预算保持原值，列容量不等于允许用户提交的容量，单条传输仍受 MySQL max_allowed_packet 等限制。原始 BLOB 数据保留编码、换行和空字节。普通 BLOB 的 65,535 字节容量不能覆盖当前 65,536 字节边界，见 [MySQL 类型容量](https://dev.mysql.com/doc/refman/8.4/en/storage-requirements.html)。

LONGTEXT 也覆盖终态 Base64 JSON 和固定 32 KiB 的审查详情。当前无需按 JSON 内部字段检索，保留文本列以复用现有解码和预算；SQL 采用 JSON_VALID 及受保护的 `JSON_TYPE(...)= 'OBJECT'` 检查，CASE 结果以 `= 1` 转为布尔条件，具体结果形状继续由应用校验。

### 4.2 外键、状态约束和索引

保留六条现有外键，引用完整主键，采用原有 RESTRICT 语义：某个父键仍被其他记录引用时，拒绝删除父记录或更改该键。例如提交引用题目版本 1 时，不能删除该版本或更改其版本身份。RESTRICT 不禁止正常更新提交状态，也不禁止把当前发布指针指向本题另一个合法版本。该规则与现有 SQLite SQL 一致，见 [MySQL 外键说明](https://dev.mysql.com/doc/refman/8.4/en/create-table-foreign-keys.html)。

循环关系按以下顺序建表：

1. 创建 users 和暂不带当前版本指针外键的 problems。
2. 创建 problem_versions、语言、用例和提交表。
3. 使用 ALTER TABLE 添加 problems 到版本表的联合外键。
4. 创建索引，最后写入结构版本行。

保留机器终态字段关系、审查字段关系、枚举和 JSON 对象检查。M01 已在实际 MySQL 8.4.11 上验证正值 CHECK 与 RESTRICT 外键组合；后续出现影响合同的兼容性问题时修订设计，不直接删除业务约束。

保留用户历史、版本语言关联和 verdict 历史索引；排队索引改为 `(state, created_at_ms, id)`，覆盖启动查询。联合主键已有的关联前缀不重复建索引，不预建所有筛选组合。

MySQL 常规 DDL 会隐式提交，初始建表不承诺整个脚本作为一个事务回滚。失败时保留错误，部署端在空库中重新执行初始化，不由应用修复半成品结构，见 [DDL 提交说明](https://dev.mysql.com/doc/refman/8.4/en/implicit-commit.html)。

### 4.3 D04 已确认的差异

D04 的三项行为／职责差异已由用户确认，分别如下。

| 项目 | 原设计 | 已确认方案及影响 |
| --- | --- | --- |
| 用户名长度 | SQLite TEXT 没有 255 字符限制 | 改为 VARCHAR(255) 以建立完整唯一键；目前只有预置测试用户，不影响当前判题流程，但未来用户名需遵守此上限 |
| 自增 ID 正值 | SQLite 在主键上附 CHECK(id > 0) | MySQL 自增列不能用于同样的 CHECK；使用自增主键，预置 SQL 与后端输入边界保证正值，不添加触发器。数据库不再用该 CHECK 拒绝所有人工写入的非正 ID |
| 普通查询网络失败 | 原实现将 SQLx I/O 错误视为严重存储故障 | MySQL 网络断连按一次普通查询失败处理，当前查询返回错误但服务继续；关键写入失败仍退出，详见第 6 节 |

自增列限制依据 [MySQL CHECK 文档](https://dev.mysql.com/doc/refman/8.4/en/create-table-check-constraints.html)。

## 5. 运行期提交和审查事务

本节处理服务运行期间的业务记录读写。创建数据库和表的首次部署操作在第 7 节；本节的“创建提交记录”发生在用户调用 `POST /api/v1/submissions` 时。

### 5.1 创建提交记录与幂等竞争

保留 CreationService 的预查询、名额预留和独立短操作生命周期。数据库不持有队列许可；事务提交并归还连接后才交接，客户端断开不取消已登记操作。

数据库创建提交记录使用 READ COMMITTED 短事务：

1. 按 `(user_id, create_request_key)` 查询已有提交，存在时比较题号、语言和源码，沿用原 ID 与原版本任务装配。
2. 不存在时，读取当前发布指针，装配该版本的源码、限额和用例，执行普通 INSERT。
3. INSERT 成功则提交，返回新提交和任务。
4. INSERT 返回 **1062** 时，回滚事务并释放其锁，在新短事务中按同一用户与幂等键读取一次记录。找到后共用内容比较／装配流程；内容相同返回原 ID，不同返回 IdempotencyConflict。
5. 该键仍不存在时保留原 1062 为关键存储错误；读取或回滚失败也明确报错，不将其他唯一键冲突当作成功。

1062 表示重复主键或唯一键（ER_DUP_ENTRY）。本场景可能是两个请求同时未查到记录，随后一个已插入，另一个 INSERT 被唯一键拒绝。上述分支只读取既有记录，不再次 INSERT；正常幂等竞争在故障通知前转为业务结果，见 [MySQL 错误参考](https://dev.mysql.com/doc/mysql-errors/8.4/en/server-error-reference.html)。

READ COMMITTED 让各次读取使用当时已提交的数据；它本身不能防止两个请求同时认为记录不存在，唯一键仍是最终约束。已有 queued 重传可继续交接，Scheduler 按 ID 合并，分派前条件 UPDATE 排除过期任务；队列满仍不新建记录。

### 5.2 手动审查接纳与完成

“手动审查”指用户手动触发 LLM 代码审查，审查本身仍由模型执行。本节处理机器判题已经 finished 后，用户点击“启动审查”或“重新审查”的场景，包括 CE、首例失败或题目关闭自动审查的提交。

例如两个页面几乎同时请求审查同一提交：两个操作都可能先预留名额，但数据库最终只能接纳同一个 PENDING。使用一个短事务，按提交主键执行 `SELECT ... FOR UPDATE`；该行在事务结束前不能被另一个接纳事务同时修改。后一个请求取得锁时读取最新状态，复用前一个已经写入的 PENDING，多余许可释放。

判断顺序保持原样：不存在 → 非 finished → 复用已有 PENDING → 比较 expected_review_id → 检查已预留名额 → 写入新 ID 和 PENDING。expected_review_id 表示页面上看到的上一次审查，防止旧页面覆盖另一轮结果。事务结束并归还连接后才交接审查，不在行锁期间调用模型，见 [MySQL 锁定读取](https://dev.mysql.com/doc/refman/8.4/en/innodb-locking-reads.html)。

完成审查沿用原短事务：读取不可变源码并校验行号，以 `id + review_id + PENDING` 为条件 UPDATE，然后提交。读取源码不另加写锁；0 行是正常的条件未生效，迟到结果不能覆盖重跑意见。

接纳、模型执行和结果保存分为以下阶段，与[既有审查执行合同](08-llm-code-review-design.md#72-队列与执行)一致：

| 阶段 | 操作 | 数据库资源边界 |
| --- | --- | --- |
| 接纳短事务 | 锁定提交行、判断状态、写入新 review_id 和 PENDING，然后 COMMIT | 提交释放行锁，归还连接后才交接队列；复用已有 PENDING 时也先结束事务 |
| 排队和模型执行 | 轮到任务时短暂读取固定源码、题面和 verdict 到内存，归还连接，再发起并等待 LLM 请求 | 排队与模型请求期间不持有数据库事务、行锁或借出的连接；审查并发由已有队列／运行名额控制 |
| 完成短事务 | LLM 返回后重新获取连接，读取源码校验结果，按本轮 ID 和 PENDING 条件写入，再 COMMIT | 只在保存阶段占用连接及写入所需的锁，结束后归还连接 |

PENDING 是已经持久化的业务状态，不表示数据库事务仍然打开。模型等待期间，页面查询和其他数据库操作可以继续使用连接池。

### 5.3 判题终态、发布与重启

- 非终态转换继续要求原状态符合条件；机器终态和自动审查初态仍保存在同一原子 UPDATE 中，首次生效才交接自动审查。
- 保存成功并归还连接后 ACK 和释放节点，再交接审查；未确认保存时不 ACK，模型等待不占数据库连接。
- 题目版本、语言、用例和发布指针仍在一个 DML 事务中写入。创建提交以读到的发布指针固定版本，历史版本不随新发布变化。
- 正常重启中断上一次运行遗留的任务并装入 queued，发生在开放接纳前；服务正常停机和故障退出沿用既有边界。

### 5.4 与原 SQLite 实现的对照

| 项目 | SQLite 实现 | MySQL 方案 | 业务差异 |
| --- | --- | --- | --- |
| 共享访问 | 单连接，写事务使用 BEGIN IMMEDIATE | 默认最多 4 个连接，READ COMMITTED 和局部行锁 | 可同时处理更多数据库操作，不改变请求与队列合同 |
| 同键并发创建 | 写事务串行后查重 | 唯一键裁定竞争，1062 后读取已保存提交 | 同内容仍一个提交，不同内容仍冲突 |
| 创建与发布并发 | 短写事务期间其他发布写入等待 | 发布可并行提交，创建仍使用已经读到的版本指针 | 版本在读取指针时固定，不追随之后发布的新版本 |
| 手动接纳审查 | 在 SQLite 短写事务内读取再更新 | 在短事务内锁定该提交行再判断 | 判断顺序、PENDING 复用和 expected_review_id 不变 |
| 完成审查 | 短事务读取源码、条件写入并提交 | 保留短事务和同样的 ID／状态条件 | 最新一次意见与迟到结果处理不变 |
| 判题结果及 ACK | 条件保存机器结果和审查初态，成功后 ACK | 保留相同写入和交接顺序 | 不变 |

这是数据库并发实现的适配，不新增业务状态或恢复流程。第 6 节的普通只读网络错误分类是已确认的 D04 差异。

## 6. 错误与健康观测

使用 MySqlDatabaseError::number() 匹配数值错误号，不再对 SQLSTATE 应用 SQLite 的低字节算法，见 [SQLx 错误类型](https://docs.rs/sqlx-mysql/0.8.6/sqlx_mysql/struct.MySqlDatabaseError.html)。

| 场景 | 行为 |
| --- | --- |
| 已确认的幂等竞争、业务拒绝、条件更新 0 行 | 正常业务结果，不触发故障退出 |
| 普通只读的池等待、网络断连、协议或服务器查询错误 | 当前查询失败，服务继续；健康探测保存失败快照；网络错误处理按已确认的 D04 方案 |
| 创建、状态转换、终态和审查关键写入错误，含锁等待 1205、死锁 1213、COMMIT 结果不明 | 结束适用事务并通知入口故障退出，未确认保存的结果不 ACK，不重发判题或模型请求 |
| 服务端明确报告表空间耗尽、存储 I/O 错误或损坏 | 读写均触发退出；初始映射为 1114、1712、1810、1811、1812、1817，保留错误上下文 |

普通网络 I/O 不等于服务端磁盘损坏；严重错误依据 [MySQL 错误参考](https://dev.mysql.com/doc/mysql-errors/8.4/en/server-error-reference.html)识别，不把全部 HY000 视为严重故障。

COMMIT 结果不明的例子：后端发出 COMMIT，MySQL 已成功提交，但成功响应返回前连接中断。后端只收到断连错误，无法断定“已提交”还是“未提交”。此时不声称回滚一定成功，不自动重复写入；记录故障并退出，后续重启依据实际保存状态处理。

死锁和锁等待错误可能有不同的回滚范围，调用方结束适用事务，沿用关键写入失败退出合同，不增加通用重试，见 [InnoDB 错误处理](https://dev.mysql.com/doc/refman/8.4/en/innodb-error-handling.html)。

健康 DTO 及缓存语义不变。探测仍通过共享池读取实际 users 表，并沿用健康检查期限；健康 GET 不额外访问数据库，不暴露端点、用户、密码或服务器原始异常。

## 7. SQL 改写与全新初始化

沿用现有 SQL 文件名并直接转换方言：

1. sql/001-initial-schema.sql：六张业务表、版本表、外键、约束和索引，版本从 4 开始。
2. examples/seed-user.sql：保留现有开发用户身份和不可登录的密码哈希。
3. sql/002-example-problem.sql：直接改写为 MySQL SQL，保留题面、语言限额及当前 104 个用例，字节值采用明确的二进制字面量。

整体重新部署的初始化流程：准备空 MySQL 数据库 → 执行初始结构 → 执行开发用户 → 执行示例题 → 提供配置与密码变量 → 启动后端及 Worker、前端 → 完成一份公开样例提交与结果查询。SQL 脚本不清空已有业务库，目标库需先为空。

以下命令针对已转换的 MySQL SQL，目标必须为独立空库；客户端默认在首个错误时停止，不使用 `--force`。先由开发／部署端准备数据库及具备所需 DDL、DML 权限的初始化账户。账户文件提供连接端点、用户和密码；后端使用自己的配置和进程密码变量。M02–M04 完成前，当前后端仍不能连接此库，不能使用 SQLite CLI 执行这些脚本。

```sh
mysql --defaults-extra-file=/path/to/mysql-client.cnf --ssl-mode=DISABLED --database=appdb < sql/001-initial-schema.sql
mysql --defaults-extra-file=/path/to/mysql-client.cnf --ssl-mode=DISABLED --database=appdb < examples/seed-user.sql
mysql --defaults-extra-file=/path/to/mysql-client.cnf --ssl-mode=DISABLED --database=appdb < sql/002-example-problem.sql
```

## 10. 可执行任务与待办清单

M01–M04 构成 MySQL 运行闭环的同一代码交付批次，各步随合同变化同步文档。任务编号沿用现有编号；实际执行和检查结果写入[实施记录](d02-execution/09-mysql-migration-execution.md)，只有完成条件实际满足才勾选。当前 M01 已完成，其他任务未开始。

### 10.1 M01：MySQL 结构和预置 SQL

依赖：D01／D03 及字段差异 D04 已确认。产物为三个 MySQL SQL 脚本及第 02 篇相关结构合同。

- [x] 将初始结构改为 InnoDB、版本 4，落实字段、外键、CHECK 和索引。
- [x] 直接转换开发用户与示例题 SQL，保留题面、限额及 104 个用例的原始内容。
- [x] 在独立空 MySQL 库执行初始化，读取预置数据并完成 V01 的结构／预置部分；同步初始化说明。

完成条件：DDL 和三个脚本可在目标版本执行，约束与预置内容符合第 4／7 节；不以本步结果声称后端已接通。

### 10.2 M02：连接、配置和访问层

依赖：M01；TOML 非秘密配置与进程密码变量方式已确认。产物为 Cargo／锁文件、配置、数据库四个模块和 MySQL 测试夹具。

- [ ] 将 SQLx 驱动切换到 MySQL，移除 SQLite 运行依赖和相关入口。
- [ ] 实现端点／库名／用户名配置、进程密码读取、禁用 SSL、连接池初始化及版本检查，隐藏凭据。
- [ ] 修改数据库连接／行类型和参数解码；将测试夹具与子进程配置改为独立 MySQL 库，使用专用测试 TOML 及进程密码。
- [ ] 同步第 02／03 篇连接合同、backend README 和根中英文 README 的相关配置与命令。

完成条件：受影响目标可编译，能够通过共同连接入口读写新库；正式后端与测试均不依赖 SQLite。

### 10.3 M03：事务和故障边界

依赖：M02；沿用已确认的原业务合同及普通只读网络故障分类 D04；隔离等级使用第 3.1 节已确认的 READ COMMITTED。

- [ ] 共用原有提交的比较／装配流程，加入限定的 1062 竞争读取，实现审查行锁接纳和短事务条件完成。
- [ ] 保留终态／自动审查初态原子保存、保存后 ACK 和独立短操作生命周期，接入 MySQL 数值错误分类。
- [ ] 在实际多连接下复用并发创建、手动审查及迟到结果检查，完成 V02–V04 的对应实现检查。
- [ ] 同步第 02／03 篇事务和故障合同、第 07 篇读取失败含义及第 08 篇审查存储说明。

完成条件：第 5 节业务语义保留，第 6 节分类准确；正常幂等竞争不退出，关键写入失败不伪造保存或 ACK。

### 10.4 M04：适配现有测试并执行 Rust 检查

依赖：M02，与 M03 配合。产物为可执行的 database／workflow／配置和进程测试。

- [ ] 将 SQLite 锁注入改为 MySQL 实际机制：目标行锁验证关键写入等待；独立控制连接的 LOCK TABLES users WRITE 及会话 lock_wait_timeout 验证普通读取失败和释放后的恢复，不假设普通 SELECT 被行写锁阻塞。
- [ ] 将现有终态／审查拒写触发器适配为 MySQL SIGNAL，保留原业务断言；完成 V01–V04 的相关检查。
- [ ] 按既有生命周期停止服务、关闭池并删除本例测试库；失败残留由开发端处理，不增加清库扫描。
- [ ] 完成第 11 节的格式、编译、必需 Clippy 和相关测试，实际结果写入实施记录。

完成条件：受影响测试和 Rust 必需检查通过，未执行的实际环境检查明确保留，不扩展为故障／状态排列测试。

### 10.6 M06：有限集成验收

依赖：M03、M04。使用用户已准备的 MySQL 服务及独立验证库，按需使用获准的开发凭据。

- [ ] 按第 7 节从空库执行初始化，启动正式后端与真实 Worker，完成既有单 Worker 真实 HTTP 验收一次。
- [ ] 用受控 Responses 检查一条审查的保存和 GUI 展示，核对结果及历史页面；记录 V05 实际范围。

完成条件：MySQL 上真实判题闭环和受控审查展示通过。应用库 appdb 的实际初始化属于整体重新部署操作，不在设计审核阶段执行。

### 10.7 M07：文档和交付核查

依赖：M06。

- [ ] 核查第 02、03、07、08 篇及文档索引已按实现同步；补齐 SQL README。
- [ ] 同步 backend、根英文／简体中文 README 的功能、初始化、配置和命令；web README 仅更新受影响内容。
- [ ] 检查文档格式、编号、链接及示例语法，将实际结果和剩余工作写入独立实施记录。

完成条件：按说明可以全新初始化和启动，当前功能与已验证范围表述准确，正式实现不再提供 SQLite 支持。

## 11. 有限验收与检查命令

### 11.1 验收清单

优先适配并复用已有 database／workflow 检查，不为所有状态、判题结论和错误建立组合矩阵。

| 标识 | 必须核对的合同 | 最小方式／任务 |
| --- | --- | --- |
| V01 | 版本、完整外键和状态关系、精确文本比较、65,536 字节原始数据及超过 64 KiB 的终态 JSON 往返；预置一致 | 复用结构／数据检查和一组实际容量边界；M01、M04 |
| V02 | 多连接同键只创建一份提交；内容冲突明确；新发布后旧键固定旧版本；手动并发只接纳同一 PENDING | 复用并发创建、固定版本和审查用例；M03、M04 |
| V03 | 条件状态、终态不可覆盖、自动审查初态与保存后 ACK；重跑和迟到结果；短操作不随 HTTP 断开取消 | 复用 workflow，使用受控 gRPC／Responses；M03、M04 |
| V04 | MySQL 自身重启装入 queued 和中断遗留任务；停机；普通读取失败后恢复，关键写入失败退出且不 ACK；健康缓存 | 适配现有进程和健康用例，使用一种受控读失败及既有拒写方式；M03、M04 |
| V05 | 全新 MySQL 库上的真实判题闭环、审查保存及 GUI 展示 | 已有单 Worker 真实 HTTP 验收一次，再检查一条受控审查及相关页面；M06 |

V05 复用既有 C/Python AC、C CE、Python WA、C TLE 路线；没有相关新问题时不重复双 Worker、隔离配额专项、批量模型评测或完整 workspace 测试。

### 11.2 命令与环境

以下使用项目已有检查命令；专用测试 TOML 接口 ACORN_TEST_DATABASE_CONFIG 及 MYSQL_PASSWORD 密码读取由 M02 实现后才可使用，当前不运行这些代码检查。

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p acorn-backend --locked --lib --test config_results
ACORN_TEST_DATABASE_CONFIG=/path/to/mysql-test.toml \
  cargo test -p acorn-backend --locked --test database --test workflow -- --test-threads=1
ACORN_TEST_DATABASE_CONFIG=/path/to/mysql-test.toml ACORN_ISOLATE_BOX_ID=204 \
  cargo test -p acorn-backend --locked --test workflow real_http_worker_acceptance -- --ignored --nocapture
```

测试 TOML 提供端点、账户及连接预算，测试夹具创建独立库，不直接使用 appdb 做测试。测试文件串行调度控制库及连接数量，V02 在单例内部并发并实际使用多个连接。正式后端不需要测试账户的建库权限。

前端源码不变时，仅为 V05 执行已有 pnpm build 并检查相关页面。真实验收另需现有 Isolate、配额和专用 box 环境；环境缺失时记录未执行。各批 Rust 变更按仓库要求完成必需检查；检查已通过且没有新变化或问题时不重复执行。

## 12. 审核与实施记录入口

- [09 审核记录](d01-review/09-mysql-migration-review.md)：已确认决策、问题解释、差异和审核结论；当前无待决设计事项。
- [09 实施记录](d02-execution/09-mysql-migration-execution.md)：实际完成的工作、检查结果和剩余事项；当前 M01 已完成，M02–M04、M06、M07 未开始。

设计正文维护当前合同和待办步骤，审核讨论与实际执行结果分别存放，不将未执行的验证写成通过。

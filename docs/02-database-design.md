# 数据库设计

本文定义 MySQL 8.4／InnoDB、结构版本 4 的数据合同；迁移任务见[迁移设计](09-mysql-migration-design.md)，实际过程和验证结果见[实施记录](d02-execution/09-mysql-migration-execution.md)。

数据访问由后端[数据库模块](../crates/backend/src/database/mod.rs)提供，使用[题目类型](../crates/backend/src/problems.rs)与[提交和终态类型](../crates/backend/src/submissions.rs)。审查字段、固定版本策略和最近一次意见遵守[LLM 数据合同](08-llm-code-review-design.md#5-数据库增量)，数据库错误按[后端故障契约](03-backend-design.md#83-数据库及后台故障)报告。开发用户脚本见 [seed-user.sql](../examples/seed-user.sql)。

按[MySQL 迁移设计](09-mysql-migration-design.md)从空库初始化，不保留旧数据；结构版本 4 标记新库结构。内存限额使用 KiB，实际结果中的内存统计使用字节。

初始建表语句见 [001-initial-schema.sql](../sql/001-initial-schema.sql)，账户准备和执行顺序见[SQL 初始化说明](../sql/README.md#初始化)，初始化职责见[迁移设计第 7 节](09-mysql-migration-design.md#7-sql-改写与全新初始化)。

[002-example-problem.sql](../sql/002-example-problem.sql) 提供随正式发布保留的两数求和题及 C/Python 限额、用例，在新库建表后装入；它不改变结构版本或创建用户。

## 1. 范围与职责

使用一个 MySQL 8.4 数据库，业务表采用 InnoDB，保存用户、题库、提交历史、业务状态和有界结果。[Worker 设计](01-worker-design.md)规定执行与结果契约。仅支持全新初始化，应用不建表或自动升级；后端共享有界 MySQL 连接池，以进程环境变量提供密码，连接合同见[后端设计](03-backend-design.md#4-数据库访问与预置数据)。

API、Scheduler 与 llm_review 属于同一后端进程，见[后端设计](03-backend-design.md)。账号通过注册持久化到现有 users，登录读取凭据后在后端内存建立 Session，题目由本地预置。HTTP 对提交和审查实施登录及预期用户校验；创建、历史、详情及手动审查按认证用户限定，遵守[用户设计第 5.2 节](11-user-authentication-design.md#52-对现有业务的改动)。

| 组件       | 数据职责                                                                                                                           |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| API        | 注册和登录访问 users；创建携带认证用户，历史、详情及手动审查按该身份限定；审查交给 llm_review                                      |
| Scheduler  | 维护内存队列与 Worker 路由，持久化重要业务状态与机器终态；启动时读取排队任务；按固定题目版本判断自动门槛并原子保存审查初态后交接   |
| llm_review | 使用独立内存队列，读取固定源码、题面及 verdict，条件保存最近一次意见或失败；自动与手动接纳共用停止边界，手动短操作独立于 HTTP 连接 |
| Worker     | 通过 Kafka 接收任务及回传事件，gRPC 只读状态，不访问数据库                                                                         |

一个 Submission 最多执行一次 Worker 判题，Submission ID 同时作为 Job ID。LLM 审查可以手动重跑，每次接纳使用新的 review_id，只保存最近一次，不重新执行 Worker。源码、题面、测试输入和期望输出保存在数据库；编译产物、沙箱工作文件和运行日志不入库。日志使用英文。

新提交和普通题目查询一律使用当前最新发布版本。历史提交固定引用创建时选定的版本；显式版本查询接口供 GUI 在提交记录中展示该次提交对应的题面，不提供题目历史版本列表或切换入口。旧版本继续支持历史提交执行、题面与结果展示。不存在题目可见性或逐题访问权限配置。

## 2. 字段表示

| 类型                               | 表示                                                                                              |
| ---------------------------------- | ------------------------------------------------------------------------------------------------- |
| 用户、题目 ID                      | 有符号 `BIGINT AUTO_INCREMENT PRIMARY KEY`；Rust、HTTP 和显式预置继续使用正值 i64，不承诺连续编号 |
| Submission ID、幂等键、审查 ID     | 规范 UUID 文本，使用 `VARCHAR(36)`；Submission ID 作为不透明的 Job ID 交给 Worker                 |
| 版本身份                           | 有符号 `BIGINT`，`(problem_id, version_no)` 联合主键，版本号在题目内从 1 递增                     |
| 时间戳                             | 有符号 `BIGINT`，UTC Unix 毫秒，字段以 `_at_ms` 结尾                                              |
| 限额与统计                         | 有符号 `BIGINT`，明确使用 ms、KiB 或 bytes 等既有单位；未知统计保持缺失                           |
| 源码及测试数据                     | `LONGBLOB`，保留编码、换行及空字节；大小使用 `OCTET_LENGTH()`                                     |
| 用户名                             | `VARCHAR(255)`，完整唯一键，最多 255 字符                                                         |
| 展示名称、题目标题、题面、密码哈希 | `LONGTEXT`；密码哈希保留包含算法、参数、盐和哈希值的编码字符串                                    |
| 状态与结论                         | `VARCHAR(32)` 固定枚举，使用 `CHECK`；语言为 `VARCHAR(16)`，比较模式为 `VARCHAR(64)`              |
| 终态和审查详情                     | 可空 `LONGTEXT`，附 JSON 对象检查；复用有界应用类型，不重复保存独立的状态或结论列                 |

除明确标注可空外，字段均为 `NOT NULL`。ID 不是内容摘要或鉴权凭据。数据库结构版本统一管理，不在每条业务记录中重复保存。

文本使用 `utf8mb4 COLLATE utf8mb4_0900_bin`，枚举与唯一键比较区分大小写、重音和末尾空格。初始化会话和每条新应用连接启用 `STRICT_ALL_TABLES,NO_ENGINE_SUBSTITUTION`，避免越界写入被静默截断；应用连接设置 READ COMMITTED 及两类有限锁等待。

MySQL 不允许对自增列使用原有的正值 CHECK，显式用户／题目 ID 正值由预置 SQL 和后端输入边界保证，不增加触发器；数据库不再拒绝所有人工写入的非正主键。这与用户名 255 字符上限均为[已确认差异](09-mysql-migration-design.md#43-d04-已确认的差异)。`LONGBLOB` 覆盖 65,536 字节边界，应用接纳预算及服务端传输限制仍然生效，列容量不扩大用户可提交的数据范围。

源码、用例数量、单项和合计字节上限遵守 Worker 请求契约。运行、编译、输出、沙箱和通信门限保存在各服务的启动配置中；题目指定的单用例限额保存在版本语言表中。

## 3. 业务表

### 3.1 users

| 字段                  | 用途与约束                                            |
| --------------------- | ----------------------------------------------------- |
| `id`                  | 自动分配的整数主键                                    |
| `username_normalized` | 最多 255 字符，精确比较的完整唯一用户名，用于账号查找 |
| `display_name`        | 展示名称                                              |
| `password_hash`       | Argon2id 密码哈希的完整编码字符串                     |
| `role`                | `student` / `admin`                                   |
| `status`              | `active` / `disabled`；停用保留历史归属               |
| `created_at_ms`       | 账号创建时间                                          |

[用户数据模块](../crates/backend/src/database/users.rs)提供按规范化用户名读取凭据和创建账号的内部能力；注册输入与密码合同见[用户设计第 3 节](11-user-authentication-design.md#3-用户数据与密码)。创建只接收已校验的用户名、显示名及已计算的 PHC 哈希，ID 自增，创建时间由服务端生成，role／status 固定为 student／active。该 INSERT 不接收主键，用户名是唯一可能冲突的输入；1062 在数据库故障观察之前转换为 UsernameTaken，其他写入错误沿用关键写入边界。读取返回自有用户数据和哈希后即释放连接，密码计算不占用连接或事务。

[auth 模块](../crates/backend/src/auth.rs)使用 RustCrypto Argon2 生成和验证密码，固定初始基线 m=19456 KiB、t=2、p=1，随机盐为 16 字节、摘要为 32 字节。操作系统随机源生成盐，算法、版本、参数、盐和摘要随 PHC 保存；验证使用存储参数。不保存明文或可逆密文，不单独维护盐列，不增加密码参数配置。哈希与验证通过 Tokio spawn_blocking 执行，密码不匹配与损坏哈希或执行失败分别返回。基线依据见 [OWASP 密码存储建议](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html)。

认证接口复用以上账号读写能力，公共响应只包含 id、username 和 display_name；role、status 暂不产生权限差异。会话及 Cookie 校验由 auth 和 HTTP 层执行，不保存到 MySQL。HTTP 将认证用户的不可变 `user_id` 传入创建服务，客户端请求体不能覆盖归属。启动不要求用户表非空，账号由注册或可选开发预置创建。

### 3.2 problems

| 字段                 | 用途与约束                                       |
| -------------------- | ------------------------------------------------ |
| `id`                 | 自动分配的整数主键，同时作为展示与查询使用的题号 |
| `current_version_no` | 可空，最新发布版本号；空表示尚未发布             |
| `created_at_ms`      | 题目创建时间                                     |

`(id, current_version_no)` 通过联合外键关联 `problem_versions(problem_id, version_no)`，保证发布指针属于本题。题号没有独立编辑需求时直接使用 `id`。

普通题目列表和默认详情只关联当前版本，额外的显式版本接口可查询指定已发布版本；历史提交详情使用其固定版本。未发布题目没有可供普通查询或提交使用的版本。显式查询旧版本不代表新提交可以选择该版本。

### 3.3 problem_versions

| 字段              | 用途与约束                                                                                                                                           |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `problem_id`      | 外键关联 `problems(id)`，与 `version_no` 构成联合主键                                                                                                |
| `version_no`      | 题目内从 1 递增的正整数                                                                                                                              |
| `title`           | 版本标题                                                                                                                                             |
| `statement`       | 版本题面的 Markdown 源文；原文保存，浏览器按[前端设计](05-frontend-design.md#32-答题页面)禁用内嵌 HTML 渲染                                          |
| `comparison_mode` | 比较规则保留字段，默认 `trim_trailing_ascii_whitespace`                                                                                              |
| `review_mode`     | 自动审查模式，`DISABLED` / `ENABLE-ANY` / `ENABLE-ALL`，默认 `DISABLED`；门槛见[第 08 篇第 4.1 节](08-llm-code-review-design.md#41-根据现有结果判断) |
| `published_at_ms` | 发布时刻                                                                                                                                             |

版本、语言限额、全部测试用例和发布指针在同一事务中写入。发布分配下一个版本号并更新指针，不提供将最新指针切回旧版本的入口。首版不持久化可编辑草稿；已发布内容不可修改，修改题面、测试、限额、比较规则或自动审查模式时创建新版本。自动门槛使用提交固定版本的模式，不随最新发布指针变化。

首版 `comparison_mode` 只允许默认值：删除整个输出字节串末尾的 ASCII 空白 HT、LF、VT、FF、CR、SPACE，再逐字节比较，内部空白保持不变。Worker 执行相同固定规则，RPC 不增加未使用的规则参数；其他规则在扩展协议与执行器后启用，当前拒绝发布不支持的值。

引用版本的表使用完整的 `(problem_id, version_no)`，不另外生成版本 UUID。版本不可变固定题目数据与限额，不提供工具链版本或跨 Worker 的可复现保证。

### 3.4 problem_version_languages

| 字段                       | 用途与约束                       |
| -------------------------- | -------------------------------- |
| `problem_id`、`version_no` | 联合外键关联题目版本             |
| `language_id`              | 首版为 `c` 或 `python`           |
| `cpu_time_ms`              | 单用例 CPU 限额，正整数          |
| `wall_time_ms`             | 单用例墙钟限额，正整数           |
| `memory_kib`               | 单用例内存限额，正整数，单位 KiB |

联合主键为 `(problem_id, version_no, language_id)`。一行表示该版本允许此语言，并给出对应预算；C 与 Python 可以采用不同限额。所有 Worker 使用相同的语言约定，不在此记录 Worker 能力。

发布时校验正值及部署约定的请求上限。Worker 接纳时仍检查本地上限，配置不匹配时显式报错，不自动调整预算。没有逐用例限额覆盖。

### 3.5 test_cases

| 字段                       | 用途与约束                            |
| -------------------------- | ------------------------------------- |
| `problem_id`、`version_no` | 联合外键关联题目版本                  |
| `case_index`               | 从 1 开始的执行顺序，版本内连续且唯一 |
| `input`                    | 输入 LONGBLOB，可以为空               |
| `expected_output`          | 期望输出 LONGBLOB，可以为空           |

联合主键为 `(problem_id, version_no, case_index)`。读取时使用 `ORDER BY case_index`；组装 RPC 后由数组顺序表达相同顺序。失败用例通过 Submission 固定版本和返回的序号定位。

发布校验至少一个用例、序号连续、数量及单项和合计大小符合请求约定。相同内容的多个用例允许存在。

### 3.6 submissions

此表保存提交历史、源码、机器业务状态、唯一机器终态及最近一次 LLM 审查。运行队列与 Worker 路由常驻 Scheduler 内存，不在数据库中保存分派地址或确认状态。

| 字段                       | 用途与约束                                                                                                                                                                                                                          |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                       | UUID 主键，同时作为 Job ID                                                                                                                                                                                                          |
| `user_id`                  | 外键关联 `users(id)`，确定历史归属                                                                                                                                                                                                  |
| `problem_id`、`version_no` | 固定题目版本                                                                                                                                                                                                                        |
| `language_id`              | 固定提交语言；与版本组成联合外键，关联 `problem_version_languages` 的完整主键                                                                                                                                                       |
| `source`                   | 非空、有界的原始源码 LONGBLOB                                                                                                                                                                                                       |
| `create_request_key`       | 创建幂等键，`UNIQUE(user_id, create_request_key)`                                                                                                                                                                                   |
| `state`                    | `queued` / `dispatching` / `running` / `finished` / `failed`，创建时为 `queued`                                                                                                                                                     |
| `verdict`                  | 可空；`finished` 时为 `AC` / `WA` / `CE` / `RE` / `TLE` / `MLE` / `OLE`                                                                                                                                                             |
| `terminal_details_json`    | 可空；编译诊断、失败用例或执行失败详情，不含重复的 verdict                                                                                                                                                                          |
| `created_at_ms`            | 创建时间，供历史排序和启动时装入队列使用                                                                                                                                                                                            |
| `terminal_at_ms`           | 可空；业务终态写入时刻，不表示旧程序实际停止的时刻                                                                                                                                                                                  |
| `review_status`            | `DISABLED` / `PENDING` / `DOUBTFUL` / `PASS` / `FAILED`，默认 `DISABLED`                                                                                                                                                            |
| `review_id`                | 可空 VARCHAR(36)，每次接纳新审查生成的 UUID，用于条件完成和重跑并发控制                                                                                                                                                             |
| `review_details_json`      | 可空 LONGTEXT JSON 对象，序列化后最多 32768 UTF-8 字节，包含最近一次意见或诊断、全部元数据及错误；错误 message 为后端固定英文说明，最多 512 UTF-8 字节，完整合同见[第 08 篇第 5.2 节](08-llm-code-review-design.md#52-最近一次详情) |

`(problem_id, version_no, language_id)` 的外键同时保证版本存在和语言属于该版本，不需要再增加一份仅检查版本存在的重复外键。主键、用户、版本、语言、源码、创建幂等键和创建时间在创建后不变。

机器业务终态不可覆盖；LLM 审查状态与意见独立更新，不改变 `state`、`verdict`、`terminal_details_json` 或 `terminal_at_ms`。`verdict` 与 `review_status` 支持直接筛选，详情无需为列表展示而解析。

| 状态                                 | `verdict`    | `terminal_details_json`         | `terminal_at_ms` |
| ------------------------------------ | ------------ | ------------------------------- | ---------------- |
| `queued` / `dispatching` / `running` | `NULL`       | `NULL`                          | `NULL`           |
| `finished`，AC                       | `AC`         | `NULL`                          | 非空             |
| `finished`，CE                       | `CE`         | 编译诊断和截断标记              | 非空             |
| `finished`，用例失败                 | 对应用例结论 | 唯一 `failed_case`              | 非空             |
| `failed`                             | `NULL`       | 失败 `code` 与有界 `diagnostic` | 非空             |

AC、CE 没有失败用例；WA、RE、TLE、MLE、OLE 必须保存首个失败用例。详情不保存成功用例、未运行用例、输入或答案。资源统计拿不到时保持缺失；退出码和信号至多一种。AC 不推算整题资源统计。

JSON 使用应用侧固定类型，字节字段编码为 Base64。保存前验证协议形状、失败用例序号和有界输出；Base64 后的 JSON 长度不能直接套用 Kafka 二进制消息上限。

审查的 DISABLED 要求 ID、详情均为 NULL；其他状态只允许出现在机器 `state=finished` 的记录上，ID 与元数据非空。PENDING 包含排队和调用中；PASS/DOUBTFUL 保存有效意见；FAILED 保存错误且 result 为 NULL，可按第 08 篇保留受限诊断对象。完成使用 `WHERE id=? AND review_id=? AND review_status='PENDING'`，重跑替换旧意见，迟到结果不覆盖新一轮。不增加审查历史表，也不把审查内容混入机器终态 JSON。

### 3.7 schema_version

固定主键 `id=1` 保存一行 `version=4`，两列使用有符号 BIGINT。CHECK 限制主键为 1、版本为正值；全部表、外键及索引创建成功后才插入版本行。后端启动只检查版本，不要求预置用户存在，不逐项扫描结构，检查失败关闭池并报错。

## 4. 创建与内存队列

本节保留既有创建、队列和审查业务合同。MySQL 数据库类型、短事务、1062 竞争读取及审查行锁按[迁移设计第 5 节](09-mysql-migration-design.md#5-运行期提交和审查事务)实现和验收。队列、固定版本和条件终态保持有效，Kafka 交接及位点规则见[Kafka 设计](10-kafka-judging-design.md)。

### 4.1 创建幂等

创建接口从 HTTP 请求头 `Idempotency-Key` 读取 `create_request_key`。客户端为一次独立提交生成随机 UUID；同一提交的网络重传复用键，下一次主动提交生成新键。该键由客户端生成，Submission ID 由 API 生成，两者用途不同。

API 使用认证用户归属，校验 UTF-8 源码编码后的大小及请求键基本格式。不可变 user_id 随请求贯穿预查询、事务内查重、INSERT 和竞争重读，不同用户可独立使用相同请求键。新建前预留进程内待接纳名额，队列满时不创建记录；已有提交的幂等查询不要求新名额。创建使用 READ COMMITTED 短事务：

1. 按 `(user_id, create_request_key)` 查找 Submission。
2. 存在时，直接比较请求的题目 ID、语言和源码字节；相同则返回原 Submission ID，内容不同则返回 `409 IdempotencyConflict`。不根据当前发布指针重选旧提交的版本。
3. 不存在时，读取该题最新版本，校验语言、限额、测试数据及包含源码的请求合计大小，生成 Submission ID 并写入源码和 `queued` 状态。
4. INSERT 返回 1062 时先回滚原事务，再在新短事务中按同一用户和幂等键读取一次，共用已有提交的内容比较和固定版本装配流程。内容一致返回原提交，不一致返回冲突；该键不存在时保留原 1062，读取或回滚失败也报告实际错误，不把其他唯一键冲突当作幂等成功。
5. 提交并归还连接后才返回可交接任务；内容冲突先结束只读事务，多余名额由调用方释放。唯一约束裁定并发竞争，不依赖应用先查一次即可阻止竞争，也不在 1062 分支再次 INSERT。

新提交请求只指定题目，不接受旧版本选择。相同键的重传仍是原提交，因此保留其原版本；发布新版本后的新键提交才绑定新版本。创建响应保证返回同一 Submission ID，当前业务状态可以变化，不持久化整份 HTTP 响应。

### 4.2 API 投递与正常调度

事务提交后，API 通过进程内交接向 Scheduler 传递完整任务数据：Submission ID、固定版本引用、语言、源码、有序测试数据和运行限额。复用创建请求中的源码字节，Scheduler 不再为该新任务回数据库读取源码或题库。交接不等待 Worker 空闲或判题完成，预留名额及并发查重顺序见后端设计。

Scheduler 使用内存队列保存待执行任务，以 Submission ID 合并重复投递。发送前确认记录仍允许发布，broker 确认后才条件更新 queued → dispatching。运行或终态可能先于投递确认到达，后续更新不得回退状态；阶段和逐例进度不写数据库。

进程可能在数据库提交后、入队前退出，已经保存的记录保持 `queued`。客户端重发同键请求时，API 将尚为 `queued` 且未在内存中的原任务再次交接，不创建新提交；装配重传数据时使用该记录的固定版本。已有排队记录因内存队列满而不能交接时，返回带原 Submission ID 的可重试错误，不回退其他状态。不存在常驻数据库补扫；尚未交接的排队记录在后端下一次启动时装入队列。

### 4.3 分派与终态

1. 从内存队列取得完整任务，不持有数据库连接地发送到 Kafka；临时发送失败或结果不明保留任务及许可重试。
2. broker 确认后以 queued 为条件写 dispatching。合法 RUNNING 允许 queued/dispatching → running；数据库提交后归还对应许可，后续合法运行快照不重复写库。
3. 首个合法终态允许 queued/dispatching/running → finished/failed，在同一事务保存结论、详情、终态时间及固定版本策略下的自动审查初态；重复或迟到终态不覆盖结果、不新建审查。
4. 提交事务并归还连接后，尝试异步入队本条事件 offset+1，再完成已保存审查的本地交接并清除 ID。入队同步失败仍先完成交接，再向监督方报告错误；保存失败不提交事件位点。

消息发布确认与位点请求入队是不同边界，生产路径不读回或逐条等待位点回执。重放沿用条件状态更新和首个有效终态，不自动重审。数据库事务不跨网络等待；不保存 Worker 路由字段、持久确认队列或 outbox。

### 4.4 手动审查接纳

审查服务先持有独立名额的预留结果，将认证 user_id 传入独立短操作，在 READ COMMITTED 短事务内以提交 id 和 user_id 执行 SELECT FOR UPDATE 锁定归属行，再读取机器状态与最新审查。不存在或不属于该用户统一返回 SubmissionNotFound；非 finished 拒绝；已有 PENDING 返回同次审查；其余先检查 expected_review_id，再检查是否取得名额，满足时写新的 ID 和 PENDING。失败分支不替换旧意见，复用和新接纳均在事务提交并归还连接后才返回并交接。

队列名额与短操作生命周期由审查服务持有，数据库不等待队列，也不持有资源许可。公共条件 start_review 复用该事务流程，只返回是否新接纳；HTTP 服务使用明确的业务结果区分复用、过期快照、容量和未完成状态。完成仍使用 review_id 条件写入，不改变机器终态。详细行为见[手动接口合同](08-llm-code-review-design.md#81-接口)。

## 5. Scheduler 启动

Scheduler 只有一个活动实例，随后端进程启动。`judging_recovery` 面向所有用户，按 created_at_ms、id 装配 queued 的固定版本完整任务，仅读取 dispatching/running 的 ID；queued 与 dispatching 合计超出待接纳容量时报错，由部署端调整配置。Scheduler、Kafka 和自动审查继续按提交 ID 访问可信内部任务；HTTP 详情专用查询以同一 SQL 的 id 和 user_id 条件读取，历史始终以 user_id 过滤，两类读取入口明确区分。

`Scheduler::restore` 为 queued、dispatching 分别恢复待接纳许可及去重记录；只将 queued 放入待发队列，dispatching 不重发；running 保留等待 ID 而不占待接纳名额。恢复读取不改写机器状态，不把后端重启当作 Worker 执行失败。消费组从 broker 已保存的事件位点继续，数据库已保存但位点落后的记录按幂等合同重读。

启动时另外将遗留 PENDING 审查保存为 FAILED/ReviewInterrupted，保留原审查 ID 和请求元数据并补完成时间，不补投模型请求，不改变机器终态。完整启动及停机编排见[后端生命周期](03-backend-design.md#8-启动停机与故障)。

## 6. 外键、约束与索引

### 6.1 外键

MySQL 结构保留下列六条完整外键，使用 `ON DELETE RESTRICT` / `ON UPDATE RESTRICT`，避免删除或更改仍被历史引用的身份：

| 子表字段                                            | 父表字段                                                         |
| --------------------------------------------------- | ---------------------------------------------------------------- |
| `problem_versions(problem_id)`                      | `problems(id)`                                                   |
| `problems(id, current_version_no)`                  | `problem_versions(problem_id, version_no)`                       |
| `problem_version_languages(problem_id, version_no)` | `problem_versions(problem_id, version_no)`                       |
| `test_cases(problem_id, version_no)`                | `problem_versions(problem_id, version_no)`                       |
| `submissions(user_id)`                              | `users(id)`                                                      |
| `submissions(problem_id, version_no, language_id)`  | `problem_version_languages(problem_id, version_no, language_id)` |

建表先创建不带发布指针外键的 problems，待 problem_versions 及其他业务表存在后通过 ALTER TABLE 添加该外键。预置及正常发布仍先插入 `current_version_no=NULL` 的题目，再写版本与子记录，最后更新指针，不关闭外键检查。版本、用例和限额在发布后不可变；历史提交不随发布指针更新。外键引用完整主键，见 [MySQL 外键说明](https://dev.mysql.com/doc/refman/8.4/en/create-table-foreign-keys.html)。

MySQL DDL 会隐式提交，初始脚本不承诺整体回滚。客户端必须在首个错误时停止；部署端针对空库重新初始化，不由应用修复半成品结构。预置用户和题目分别使用 DML 事务。

### 6.2 约束与索引

主键与唯一键约束 ID、用户名、创建幂等键和版本内语言/用例。MySQL 会话采用严格模式；SQL 检查固定枚举、既有正值限额和序号、非空源码、终态详情的 JSON 对象格式，以及第 3.6 节定义的终态空值关系。JSON CHECK 先用 CASE 与 JSON_VALID 保护 JSON_TYPE 调用，要求非空详情为 OBJECT；CASE 结果通过 `= 1` 转为 MySQL CHECK 要求的布尔条件。应用校验连续用例序号、字节上限、支持的比较规则和具体结果形状；整数转换保持有符号 i64 范围。

SQL 同时约束审查枚举、JSON 对象格式和基本空值关系；详情类型与内容预算由应用在组装及写入边界按第 08 篇第 5.2 节校验。模型内容或返回元数据造成详情超限时保存 FAILED/ReviewInvalidOutput，使用预算内的失败详情，不作为数据库写入故障。历史查询按 review_status 与既有条件取交集，复用现有索引，首版不预建额外审查索引。

初始查询索引：

| 索引                                               | 目的                             |
| -------------------------------------------------- | -------------------------------- |
| 各主键与唯一键对应索引                             | 身份、版本、创建幂等键及用例读取 |
| `submissions(user_id, created_at_ms, id)`          | 用户提交历史                     |
| `submissions(problem_id, version_no, language_id)` | 版本/语言关联和引用检查          |
| `submissions(verdict, created_at_ms, id)`          | 按判题结论直接检索               |
| `submissions(state, created_at_ms, id)`            | 按 queued 状态及顺序装入排队任务 |

联合主键已覆盖版本子表的关联前缀，无需重复建立同前缀索引。Scheduler 不按每次进度或心跳更新数据库。

## 7. 最小实施验证

验证单库发布与提交引用、最新版本选择和历史版本固定、认证用户归属与两用户数据隔离、并发同键只创建一个提交、API 直接交接与重复交接合并、明确忙时退回内存队列、AC/CE/首个失败用例的结果存储，以及按 verdict 检索。用户数据与密码能力验证账号创建／读取、同名并发的唯一索引裁决、重名不通知致命故障及预置密码；HTTP 登录、会话、来源校验、个人归属和空用户表启动按[用户设计](11-user-authentication-design.md)验收。

Scheduler 重启验证 queued 装入内存、dispatching/running 恢复等待及正确许可；重读事件不覆盖首个有效终态。位点请求入队晚于业务提交，入队失败仍完成已保存审查交接；字节往返不损坏，未知统计保持缺失。

数据与事务验收要求见[迁移验收 V01–V05](09-mysql-migration-design.md#111-验收清单)，实际执行及覆盖范围见[迁移实施记录](d02-execution/09-mysql-migration-execution.md)。历史 LLM 数据检查见[审查实施记录](d02-execution/08-llm-code-review-execution.md#121-l01数据契约与离线升级)。

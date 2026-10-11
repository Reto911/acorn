# 数据库设计

状态：已确认，版本 1。

初始建表语句见 [001-initial-schema.sql](../sql/001-initial-schema.sql)，执行说明见 [SQL 使用说明](../sql/README.md)。

## 1. 范围与职责

使用一个 SQLite 数据库 `acorn.db`，保存用户、题库、提交历史、业务状态和有界结果。[Worker 设计](01-worker-design.md)规定执行与结果契约。

API 与 Scheduler 属于同一后端进程，见[后端设计](03-backend-design.md)。当前只提供判题闭环，使用部署预置并由配置指定的单一测试用户，不启用登录鉴权；题目也由本地预置。

| 组件 | 数据职责 |
| --- | --- |
| API | 使用配置的测试用户，查询最新题目版本，创建提交并保存源码，通过进程内交接将任务数据交给 Scheduler，查询提交历史 |
| Scheduler | 维护内存队列与 Worker 路由，持久化重要业务状态与终态；启动时读取尚未分派的排队任务 |
| Worker | 通过 gRPC 接收任务、返回状态和结果，不访问数据库 |

一个 Submission 最多执行一次，Submission ID 同时作为 Job ID。源码、题面、测试输入和期望输出保存在数据库；编译产物、沙箱工作文件和运行日志不入库。日志使用英文。

新提交和普通题目查询一律使用当前最新发布版本。历史提交固定引用创建时选定的版本；额外提供显式版本查询接口，前端不默认接入。旧版本继续支持历史提交执行、题面与结果展示。不存在题目可见性或逐题访问权限配置。

## 2. 字段表示

| 类型 | 表示 |
| --- | --- |
| 用户、题目 ID | 使用 SQLite `INTEGER PRIMARY KEY` 自动分配正整数；正常插入递增，不承诺连续编号或已删除编号永不复用 |
| Submission ID | API 生成的 UUID，存为 `TEXT PRIMARY KEY`，作为不透明的 Job ID 交给 Worker |
| 版本身份 | `(problem_id, version_no)` 联合主键，版本号在题目内从 1 递增 |
| 时间戳 | `INTEGER`，UTC Unix 毫秒，字段以 `_at_ms` 结尾 |
| 限额与统计 | `INTEGER`，明确使用毫秒、微秒或字节；未知统计保持缺失 |
| 源码及测试数据 | `BLOB`，保留原始字节；大小需要时使用 `length()` |
| 题面、密码哈希 | `TEXT`；密码哈希使用包含算法、参数、盐和哈希值的编码字符串 |
| 状态与结论 | `TEXT` 固定枚举，使用 `CHECK` |
| 终态详情 | 有界、类型化 JSON，不重复保存独立的状态或结论列 |

除明确标注可空外，字段均为 `NOT NULL`。ID 不是内容摘要或鉴权凭据。数据库结构版本统一管理，不在每条业务记录中重复保存。

用户与已发布题目不提供硬删除入口，因此首版不要求 `AUTOINCREMENT` 的永不复用保证；整数自动分配的语义见 [SQLite 官方说明](https://www.sqlite.org/autoinc.html)。

源码、用例数量、单项和合计字节上限遵守 Worker 请求契约。运行、编译、输出、沙箱和通信门限保存在各服务的启动配置中；题目指定的单用例限额保存在版本语言表中。

## 3. 业务表

### 3.1 users

| 字段 | 用途与约束 |
| --- | --- |
| `id` | 自动分配的整数主键 |
| `username_normalized` | 唯一用户名，用于账号查找 |
| `display_name` | 展示名称 |
| `password_hash` | Argon2id 密码哈希的完整编码字符串 |
| `role` | `student` / `admin` |
| `status` | `active` / `disabled`；停用保留历史归属 |
| `created_at_ms` | 账号创建时间 |

用户表与密码哈希字段保留，当前后端不提供账号创建或密码验证。启用账号功能时，使用成熟库生成带随机盐的 Argon2id 哈希并验证，不保存明文或可逆密文，也不单独维护盐列。参数从 API 配置读取；基线至少使用 19 MiB 内存、2 次迭代、并行度 1，见 [OWASP 密码存储建议](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html)。哈希的算法和参数随编码保存。

当前 API 从后端配置取得预置测试用户的 `user_id`，启动时检查其存在，不接受客户端指定身份覆盖提交归属。后续启用登录时再由已验证的登录上下文提供归属；登录会话形式不由 Worker 或本表决定。

### 3.2 problems

| 字段 | 用途与约束 |
| --- | --- |
| `id` | 自动分配的整数主键，同时作为展示与查询使用的题号 |
| `current_version_no` | 可空，最新发布版本号；空表示尚未发布 |
| `created_at_ms` | 题目创建时间 |

`(id, current_version_no)` 通过联合外键关联 `problem_versions(problem_id, version_no)`，保证发布指针属于本题。题号没有独立编辑需求时直接使用 `id`。

普通题目列表和默认详情只关联当前版本，额外的显式版本接口可查询指定已发布版本；历史提交详情使用其固定版本。未发布题目没有可供普通查询或提交使用的版本。显式查询旧版本不代表新提交可以选择该版本。

### 3.3 problem_versions

| 字段 | 用途与约束 |
| --- | --- |
| `problem_id` | 外键关联 `problems(id)`，与 `version_no` 构成联合主键 |
| `version_no` | 题目内从 1 递增的正整数 |
| `title` | 版本标题 |
| `statement` | 版本题面正文 |
| `comparison_mode` | 比较规则保留字段，默认 `trim_trailing_ascii_whitespace` |
| `published_at_ms` | 发布时刻 |

版本、语言限额、全部测试用例和发布指针在同一事务中写入。发布分配下一个版本号并更新指针，不提供将最新指针切回旧版本的入口。首版不持久化可编辑草稿；已发布内容不可修改，修改题面、测试、限额或比较规则时创建新版本。

首版 `comparison_mode` 只允许默认值：删除整个输出字节串末尾的 ASCII 空白 HT、LF、VT、FF、CR、SPACE，再逐字节比较，内部空白保持不变。Worker 执行相同固定规则，RPC 不增加未使用的规则参数；其他规则在扩展协议与执行器后启用，当前拒绝发布不支持的值。

引用版本的表使用完整的 `(problem_id, version_no)`，不另外生成版本 UUID。版本不可变固定题目数据与限额，不提供工具链版本或跨 Worker 的可复现保证。

### 3.4 problem_version_languages

| 字段 | 用途与约束 |
| --- | --- |
| `problem_id`、`version_no` | 联合外键关联题目版本 |
| `language_id` | 首版为 `c` 或 `python` |
| `cpu_time_ms` | 单用例 CPU 限额，正整数 |
| `wall_time_ms` | 单用例墙钟限额，正整数 |
| `memory_bytes` | 单用例内存限额，正整数 |

联合主键为 `(problem_id, version_no, language_id)`。一行表示该版本允许此语言，并给出对应预算；C 与 Python 可以采用不同限额。所有 Worker 使用相同的语言约定，不在此记录 Worker 能力。

发布时校验正值及部署约定的请求上限。Worker 接纳时仍检查本地上限，配置不匹配时显式报错，不自动调整预算。没有逐用例限额覆盖。

### 3.5 test_cases

| 字段 | 用途与约束 |
| --- | --- |
| `problem_id`、`version_no` | 联合外键关联题目版本 |
| `case_index` | 从 1 开始的执行顺序，版本内连续且唯一 |
| `input` | 输入 BLOB，可以为空 |
| `expected_output` | 期望输出 BLOB，可以为空 |

联合主键为 `(problem_id, version_no, case_index)`。读取时使用 `ORDER BY case_index`；组装 RPC 后由数组顺序表达相同顺序。失败用例通过 Submission 固定版本和返回的序号定位。

发布校验至少一个用例、序号连续、数量及单项和合计大小符合请求约定。相同内容的多个用例允许存在。

### 3.6 submissions

此表保存提交历史、源码、业务状态和唯一终态。运行队列与 Worker 路由常驻 Scheduler 内存，不在数据库中保存分派地址或确认状态。

| 字段 | 用途与约束 |
| --- | --- |
| `id` | UUID 主键，同时作为 Job ID |
| `user_id` | 外键关联 `users(id)`，确定历史归属 |
| `problem_id`、`version_no` | 固定题目版本 |
| `language_id` | 固定提交语言；与版本组成联合外键，关联 `problem_version_languages` 的完整主键 |
| `source` | 非空、有界的原始源码 BLOB |
| `create_request_key` | 创建幂等键，`UNIQUE(user_id, create_request_key)` |
| `state` | `queued` / `dispatching` / `running` / `finished` / `failed`，创建时为 `queued` |
| `verdict` | 可空；`finished` 时为 `AC` / `WA` / `CE` / `RE` / `TLE` / `MLE` / `OLE` |
| `terminal_details_json` | 可空；编译诊断、失败用例或执行失败详情，不含重复的 verdict |
| `created_at_ms` | 创建时间，供历史排序和启动时装入队列使用 |
| `terminal_at_ms` | 可空；业务终态写入时刻，不表示旧程序实际停止的时刻 |

`(problem_id, version_no, language_id)` 的外键同时保证版本存在和语言属于该版本，不需要再增加一份仅检查版本存在的重复外键。主键、用户、版本、语言、源码、创建幂等键和创建时间在创建后不变。

业务终态不可覆盖。`verdict` 支持直接检索和筛选，终态详情无需为列表展示而解析。

| 状态 | `verdict` | `terminal_details_json` | `terminal_at_ms` |
| --- | --- | --- | --- |
| `queued` / `dispatching` / `running` | `NULL` | `NULL` | `NULL` |
| `finished`，AC | `AC` | `NULL` | 非空 |
| `finished`，CE | `CE` | 编译诊断和截断标记 | 非空 |
| `finished`，用例失败 | 对应用例结论 | 唯一 `failed_case` | 非空 |
| `failed` | `NULL` | 失败 `code` 与有界 `diagnostic` | 非空 |

AC、CE 没有失败用例；WA、RE、TLE、MLE、OLE 必须保存首个失败用例。详情不保存成功用例、未运行用例、输入或答案。资源统计拿不到时保持缺失；退出码和信号至多一种。AC 不推算整题资源统计。

JSON 使用应用侧固定类型，字节字段编码为 Base64。保存前验证协议形状、失败用例序号和有界输出；Base64 后的 JSON 长度不能直接套用 gRPC 二进制消息上限。

## 4. 创建与内存队列

### 4.1 创建幂等

创建接口从 HTTP 请求头 `Idempotency-Key` 读取 `create_request_key`。客户端为一次独立提交生成随机 UUID；同一提交的网络重传复用键，下一次主动提交生成新键。该键由客户端生成，Submission ID 由 API 生成，两者用途不同。

API 使用配置中的测试用户归属，校验 UTF-8 源码编码后的大小及请求键基本格式。新建前预留进程内待接纳名额，队列满时不创建记录；已有提交的幂等查询不要求新名额。创建最终由一个 `BEGIN IMMEDIATE` 短写事务保证：

1. 按 `(user_id, create_request_key)` 查找 Submission。
2. 存在时，直接比较请求的题目 ID、语言和源码字节；相同则返回原 Submission ID，内容不同则返回 `409 IdempotencyConflict`。不根据当前发布指针重选旧提交的版本。
3. 不存在时，读取该题最新版本，校验语言、限额、测试数据及包含源码的请求合计大小，生成 Submission ID 并写入源码和 `queued` 状态。
4. 提交事务。使用唯一约束和写事务处理并发的同键请求，不依赖应用先查一次即可阻止竞争。

新提交请求只指定题目，不接受旧版本选择。相同键的重传仍是原提交，因此保留其原版本；发布新版本后的新键提交才绑定新版本。创建响应保证返回同一 Submission ID，当前业务状态可以变化，不持久化整份 HTTP 响应。

### 4.2 API 投递与正常调度

事务提交后，API 通过进程内交接向 Scheduler 传递完整任务数据：Submission ID、固定版本引用、语言、源码、有序测试数据和运行限额。复用创建请求中的源码字节，Scheduler 不再为该新任务回数据库读取源码或题库。交接不等待 Worker 空闲或判题完成，预留名额及并发查重顺序见后端设计。

Scheduler 使用内存队列保存待执行任务，以 Submission ID 合并重复投递。分派前仍以 `state='queued'` 为条件持久化状态变化；条件不满足时丢弃过期投递，防止已结束的旧提交被重新执行。阶段、已完成用例数和普通轮询结果不写数据库。

进程可能在数据库提交后、入队前退出，已经保存的记录保持 `queued`。客户端重发同键请求时，API 将尚为 `queued` 且未在内存中的原任务再次交接，不创建新提交；装配重传数据时使用该记录的固定版本。已有排队记录因内存队列满而不能交接时，返回带原 Submission ID 的可重试错误，不回退其他状态。不存在常驻数据库补扫；尚未交接的排队记录在后端下一次启动时装入队列。

### 4.3 分派与终态

1. 从内存队列取任务，选择配置中的 Worker，将仍为 `queued` 的记录改为 `dispatching`，提交事务后在内存中记录目标地址并调用 `SubmitJob`。
2. 接纳成功后条件更新为 `running`，查询同一 Worker、同一 Job ID。
3. 明确收到忙、结果区满或停机且未接纳的响应时，退回 `queued`、清除该次内存路由并放回队首，暂缓该节点；有其他候选 Worker 时立即分派。节点可用状态缓存在内存，最终仍由 Worker 接纳响应确认。
4. 确定性非法请求直接失败。提交超时或断连时，按 Worker 契约做一次有界查询；无法确认或明确不存在即保存失败，不向其他 Worker 重交旧 Job。
5. 收到终态后，在同一事务中写入结论、详情、终态时间与业务状态。条件要求原状态仍为 `dispatching` 或 `running`，迟到结果不能覆盖失败。
6. 数据库提交成功后发送 ACK。保存失败时不 ACK，Worker 继续保留结果；ACK 失败记录英文日志，不回滚已保存的终态，不重新判题。重复确认使用现有幂等 RPC，不持久化确认状态。

数据库事务不跨越网络调用。退回队列只针对明确未接纳的任务，不重新执行已经开始的判题。重要状态和终态仍需写库，内存队列减少的是重复队列查询与任务读取。

Worker 地址列表来自 Scheduler 启动配置；本次任务到 Worker 的映射只保存在内存中。不设置 Worker 数据表、分派表或 Submission 的路由字段。

## 5. Scheduler 启动

Scheduler 首版只有一个活动实例，随后端进程一同启动。启动时：

1. 在短事务中将 `dispatching` 和 `running` 记录改为 `failed`，保存 `SchedulerInterrupted` 与终态时间，已有业务终态保持不变。
2. 检查现有排队数量不超过配置上限；超限则报错退出，由部署端调整上限。按 `created_at_ms, id` 读取 `queued` 记录，关联其固定版本、限额和测试数据，装入内存队列。正常运行后不轮询数据库队列。

不查询旧 Worker、不恢复执行、不重交旧 Job，也不补发旧任务的 ACK。Worker 可能仍继续执行，其本地限额负责约束；后续新任务的接纳由 Worker 判断，不能把业务失败当作执行位置已经释放。

正常运行时 Scheduler 在保存终态后发送 ACK，但其中断可能留下 Worker 未确认的记录。首版接受这一边界，不建立持久确认队列，结果区满或残留记录由部署端处理。

## 6. 外键、约束与索引

### 6.1 外键

所有数据库连接显式启用 `PRAGMA foreign_keys = ON`。下列关系使用 `ON DELETE RESTRICT` / `ON UPDATE RESTRICT`，避免删除或更改仍被历史引用的身份：

| 子表字段 | 父表字段 |
| --- | --- |
| `problem_versions(problem_id)` | `problems(id)` |
| `problems(id, current_version_no)` | `problem_versions(problem_id, version_no)` |
| `problem_version_languages(problem_id, version_no)` | `problem_versions(problem_id, version_no)` |
| `test_cases(problem_id, version_no)` | `problem_versions(problem_id, version_no)` |
| `submissions(user_id)` | `users(id)` |
| `submissions(problem_id, version_no, language_id)` | `problem_version_languages(problem_id, version_no, language_id)` |

先插入 `current_version_no=NULL` 的题目，再写版本与子记录，最后更新指针，循环关系不需要关闭外键检查。版本、用例和限额在发布后不可变；历史提交不随发布指针更新。联合外键的支持与要求见 [SQLite 外键文档](https://www.sqlite.org/foreignkeys.html)。

### 6.2 约束与索引

主键与唯一键约束 ID、用户名、创建幂等键和版本内语言/用例。表使用 `STRICT` 约束存储类型；SQL 检查固定枚举、正值限额和序号、终态详情的 JSON 对象格式，以及第 3.6 节定义的终态空值关系。应用校验连续用例序号、字节上限、支持的比较规则和具体结果形状；整数转换符合 SQLite 有符号整数范围。

初始查询索引：

| 索引 | 目的 |
| --- | --- |
| 各主键与唯一键对应索引 | 身份、版本、创建幂等键及用例读取 |
| `submissions(user_id, created_at_ms, id)` | 用户提交历史 |
| `submissions(problem_id, version_no, language_id)` | 版本/语言关联和引用检查 |
| `submissions(verdict, created_at_ms, id)` | 按判题结论直接检索 |
| `submissions(created_at_ms, id) WHERE state='queued'` | 启动时按顺序装入排队任务 |

联合主键已覆盖版本子表的关联前缀，无需重复建立同前缀索引。Scheduler 不按每次进度或心跳更新数据库。

## 7. 最小实施验证

验证单库发布与提交引用、最新版本选择和历史版本固定、配置测试用户的归属、并发同键只创建一个提交、API 直接交接与重复交接合并、明确忙时退回内存队列、AC/CE/首个失败用例的结果存储，以及按 verdict 检索。当前不验证尚未启用的登录和密码功能。

Scheduler 重启验证在途记录立即失败、排队记录装入内存且不向旧 Worker 查询或补发 ACK；运行中验证迟到结果不覆盖已保存失败。确认动作必须晚于业务终态提交；字节字段往返不损坏，未知统计保持缺失。

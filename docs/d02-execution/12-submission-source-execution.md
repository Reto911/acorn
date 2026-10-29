# 本人历史源码查看与审查行号定位实施记录

对应[设计 v1](../12-submission-source-design.md)及[审核记录](../d01-review/12-submission-source-review.md)。任务沿用 SC01–SC04，验收沿用 SV01–SV05。

## 1. 当前状态与本轮范围

整体方案已获用户批准，保留独立源码接口。SC01.1–SC01.5 已完成，后端源码接口及定向验收已交付；SC02–SC04 尚未实施，任务定义继续见[设计第 6 节](../12-submission-source-design.md#6-模块改动与实施步骤)。

SC01 交付 `GET /api/v1/submissions/{id}/source`，返回本人提交的原始源码，覆盖 SV01、SV02 的后端要求。接口沿用既有 Session、X-Acorn-User-Id、no-store 和 SQL 归属边界。数据库结构保持版本 4，前端展示和行号定位分别由 SC02、SC03 实施。

## 2. 开发清单

各项完成实现及相应检查后勾选，实际结果与遗留问题记录在[第 4 节](#4-实施记录)。

- [x] **SC01.1 定义源码读取类型与 HTTP DTO。** 在[提交类型](../../crates/backend/src/submissions.rs)新增小型 SubmissionSource，字段为 id、language_id、source，均为 String；source 表示数据库边界严格解码后的原文。在 [HTTP DTO](../../crates/backend/src/http/dto.rs)定义显式响应，只序列化 submission_id、language_id、source，并完成内部 id 的字段映射。更新 DTO 模块注释，保留详情和列表不返回源码的合同；新增 Rust 项提供 Rustdoc。无前置代码依赖。
- [x] **SC01.2 实现本人源码的最小投影查询。** 依赖 SC01.1；在[数据库提交模块](../../crates/backend/src/database/submissions.rs)新增 submission_source_for_user(user_id, id)，返回 Result<Option<SubmissionSource>>。同一参数化 SELECT 同时匹配 id 和 user_id，只读取 id、language_id、source；source 先按 Vec<u8> 读取，再经 String::from_utf8 严格解码，解码错误映射 Error::Data。通过现有 Database::observe 按普通读取观察 SQL 错误，返回自有数据并归还连接；不读取或解析判题和审查 JSON，不按业务状态限制读取，不重新检查当前源码接纳上限。
- [x] **SC01.3 接入受保护的源码 HTTP 路由。** 依赖 SC01.1、SC01.2；在 [HTTP 路由](../../crates/backend/src/http/mod.rs)的 personal 组注册 GET，处理器从 AuthenticatedUser 取得归属，按已有详情处理器解析并规范化 UUID，再调用本人源码查询。成功返回三字段 JSON；空结果使用现有 404 SubmissionNotFound，路径错误为 400 InvalidRequest，数据库或解码错误使用现有 500 InternalError。Session、预期账号及 no-store 继续由公共认证边界处理，避免重复实现校验或新增错误码。
- [x] **SC01.4 完成原文与权限的定向回归。** 依赖 SC01.2、SC01.3；按[第 3.1 节](#31-覆盖与复用入口)扩展现有认证测试，必要时增加一个源码读取的 HTTP／MySQL 用例，覆盖原文、排队与终态可读、本人隔离、认证先于参数解析、no-store 及严格解码失败。详情和列表不含源码的断言继续保留；新增或扩展检查前先告知用户覆盖内容与必要性。
- [x] **SC01.5 完成检查与后端交付说明。** 依赖 SC01.1–SC01.4；执行[第 3.2 节](#32-执行命令与环境)的必要检查，记录实际结果和未通过项。更新[后端 API 说明](../../crates/backend/README.md#http-api)的路由及携带 Cookie／预期账号的读取示例，将[后端设计](../03-backend-design.md#5-http-接口)与[用户设计](../11-user-authentication-design.md#52-对现有业务的改动)中的计划性描述同步为已接入的接口合同；[前端依赖表](../05-frontend-design.md#61-首版接口覆盖与缺口)注明后端接口已提供、页面仍按 SC02–SC03 接入。保持设计中的 SC01–SC04 标识及未实施任务的状态准确。

### 2.1 执行顺序与实现边界

按 SC01.1 → SC01.2 → SC01.3 推进，SC01.4 的对应检查随实现补充并执行，最后完成 SC01.5。该阶段可独立交付给后续前端，不依赖 Monaco 或审查定位实现。

内部类型放在既有 submissions 模块，HTTP 类型放在现有 DTO 模块；数据库方法使用设计中的静态 SQL。无需新增 crate、依赖、服务层、配置、事务、索引或数据库迁移，也不重构既有详情查询。字段与错误的外部合同继续以[设计第 3 节](../12-submission-source-design.md#3-http-与数据库合同)为准。

## 3. 定向验证与交付检查

### 3.1 覆盖与复用入口

优先使用 [authentication 测试模块](../../crates/backend/tests/cases/authentication.rs)及现有独立 MySQL／HTTP 夹具，不另建测试框架。用已有公共边界证明新路由被正确接入，避免复制会话和 Cookie 的完整测试矩阵。

| 位置                                           | 覆盖内容                                                                                                                                                                    | 必要性                                                  |
| ---------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| 扩展 authentication_routes_and_request_guards  | 将源码路径加入匿名 401 与错误预期账号 409 的现有场景；使用非法 UUID 确认身份检查优先，合法会话及匹配账号下该路径返回 400；补充缺失预期账号头的拒绝断言                      | 验证新路由没有绕过 personal 边界，沿用已批准的错误顺序  |
| 扩展 personal_submission_ownership_and_queries | 两账号分别读取本人源码成功，读取他人和不存在的源码返回相同 404；检查响应仅有约定三字段，详情和列表仍无源码                                                                  | 验证同一 SQL 限定归属，防止新增响应泄漏完整内部提交对象 |
| 新增必要的源码读取 HTTP／MySQL 场景            | 使用同时包含中文、制表符、空行、LF、CRLF、单独 CR 及末尾换行的源码，比较响应字符串重新编码后的字节与提交原文；同一提交在 queued 和通过既有 save_terminal 保存终态后均可读取 | 覆盖原文不被裁剪或转换，以及读取不依赖机器判题结束      |
| 在源码读取场景中补充严格解码检查               | 仅在独立测试库中将该记录的 source 改为非 UTF-8 字节，再读取并确认返回 500 InternalError，不返回有损替换的字符串                                                             | 覆盖新增字节到字符串转换的明确失败合同                  |

现有 request 辅助函数会检查 no-store。通过 call 辅助函数取得的源码成功及 404 响应须显式检查该头；500 场景同样检查，确保错误也沿用公共缓存策略。无需用真实 Worker 执行测试源码，或调用 Kafka、Isolate 和外部模型来证明只读接口合同。

上述场景可放在同一 authentication 模块，由已有 workflow 过滤命令运行，不再为同一合同建立重复的数据库测试集。既有[数据库二进制类型测试](../../crates/backend/tests/database.rs)仍作为 LONGBLOB 按字节读取的基础，不因新增接口而改写 SQLx 类型规则。

### 3.2 执行命令与环境

Rust 实现完成后使用设计及仓库中已有命令：

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p acorn-backend --locked --test workflow authentication
```

认证 workflow 使用既有[后端测试配置](../../config/backend-test.example.toml)的私有副本，通过 ACORN_TEST_DATABASE_CONFIG 指定，并提供进程 MYSQL_PASSWORD。沿用[后端测试说明](../../crates/backend/README.md#检查命令)的专用账户和独立库创建／清理流程；不读取或修改部署数据库，不在记录中保存密码或机器路径。

纯规划阶段只检查文档格式、章节及任务编号、本地链接和差异空白，不运行上述 Rust 命令。代码实施阶段的文档修改也须通过同类检查。

## 4. 实施记录

### 4.1 SC01 任务规划

规划阶段细化了 SC01 的五项任务及验证范围，当时未修改业务代码或执行编译、Clippy、业务测试和部署，SC01.1–SC01.5 均保持未完成。

### 4.2 SC01 后端源码接口与定向验收

SC01.1–SC01.5 已完成。新增内部 SubmissionSource 和显式三字段 DTO；database/submissions 通过单条 SELECT 同时匹配 id、user_id，只读取身份、语言和源码，将 LONGBLOB 按字节读取后严格解码为 UTF-8。新增 GET 路由加入现有 personal 组，沿用 Session、预期账号、no-store 和错误映射，源码查询不依赖判题或审查状态。既有详情及列表响应继续不含源码。

后端使用说明新增路由及 Cookie／预期账号的读取示例，第 03、05、11 篇同步接口和归属合同；前端说明明确独立后端接口可用，SC02–SC03 的只读展示及审查定位仍待接入。数据库结构、配置、Worker、Kafka 及前端源码未改变。

| 实际检查                                                            | 结果与覆盖                                                                                                                         |
| ------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| cargo fmt --all -- --check                                          | 通过                                                                                                                               |
| cargo check --workspace --all-targets --locked                      | 通过，包含受影响的后端及全部现有测试目标                                                                                           |
| cargo clippy --workspace --all-targets --locked -- -D warnings      | 通过                                                                                                                               |
| cargo test -p acorn-backend --locked --test workflow authentication | 在允许本机测试数据库连接的环境重跑后 4 项通过、40 项按过滤未执行；包含 1 项新增源码检查，以及已有认证、个人查询和 Cookie／会话检查 |
| 文档检查                                                            | 7 份文档通过 Prettier、本地链接／锚点、章节和任务编号检查，git diff --check 通过                                                   |

首次认证 workflow 在沙箱中执行，4 项均在 MySQL 测试连接阶段因 Operation not permitted 失败，尚未进入接口场景。随后重跑相同测试组通过，首次环境失败保留为本节记录，不计为接口断言失败或通过。

实际接口检查覆盖三字段响应、本人及他人／不存在归属、匿名 401、缺失或错误预期账号 409、匹配身份的错误 UUID 400，以及这些路径的 no-store。新增场景通过正式 HTTP 创建带中文、制表符、空行、LF、CRLF、单独 CR 和尾换行的源码，在 queued 与 finished 分别核对返回字节；将独立测试库中的该源码改为非 UTF-8 字节后，读取返回 500 InternalError，未生成有损字符串。

本轮终态由既有 save_terminal 在夹具中写入，不执行测试源码。结果限定于正式路由与独立 MySQL 的源码读取合同，没有运行完整 workflow、前端浏览器、真实 Kafka／Isolate、外部模型或部署验收；SC02–SC04 保持未完成。

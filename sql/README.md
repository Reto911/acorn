# 数据库 SQL

[001-initial-schema.sql](001-initial-schema.sql) 当前实现[数据库设计](../docs/02-database-design.md)中已交付的结构版本 2，包含六张业务表、外键、约束和索引。数据库结构版本记录在 `PRAGMA user_version` 中。

LLM 增量已批准、待实施：L01 将完整初始 SQL 与应用版本检查一同更新为 3，增加 review_mode、review_status、review_id 和 review_details_json。版本 2 → 3 的升级 SQL 及执行命令届时单独交付，不纳入仓库；部署端停服务、备份后离线执行，旧记录默认 DISABLED，不补跑审查，详见[第 08 篇升级合同](../docs/08-llm-code-review-design.md#53-数据库升级职责)。本文现有命令用于当前版本的新库初始化，不用于已有库升级。

使用 SQLite 3.38.0 或更新版本，并启用 JSON 函数。表使用 [STRICT 模式](https://www.sqlite.org/stricttables.html)约束存储类型；[JSON 函数](https://www.sqlite.org/json1.html)检查终态详情是合法 JSON 对象。

在仓库根目录对新的空数据库执行一次：

```sh
sqlite3 -bail acorn.db < sql/001-initial-schema.sql
```

脚本自行开启并提交事务。应用使用驱动执行时，应在尚未开始事务的连接上运行整个脚本；发生错误时回滚并报告，不继续使用未初始化完成的数据库。脚本不插入业务数据。

结构版本 2 的 `problem_version_languages.memory_kib` 以 KiB 保存内存限额，取代版本 1 的 `memory_bytes`。后端只接受结构版本 2，不自动升级；已有版本 1 数据库需由部署端离线转换或另建新库，不能直接重跑初始脚本、仅修改 `user_version` 或沿用原字节数。本文初始化命令仅适用于新库。

[002-example-problem.sql](002-example-problem.sql) 提供随正式发布保留的内置题目“两数求和（A + B）”。在新库建表后执行一次：

```sh
sqlite3 -bail acorn.db < sql/002-example-problem.sql
```

该文件是初始业务数据，不改变数据库结构版本 2。它写入题号 1、版本 1，允许 C 和 Python，包含四个按顺序执行的用例；输入和答案均为 BLOB。C 的单用例预算为 CPU 1 秒、墙钟 3 秒、内存 256 MiB；Python 为 CPU 2 秒、墙钟 5 秒、内存 256 MiB，内存列均保存 262144 KiB。发布在一个事务中完成：先创建版本指针为空的题目，再写版本、语言和用例，最后指向版本 1。

示例脚本用于新库，不覆盖已有题目。它不创建用户；首版测试用户由部署预置，后端配置其 `user_id`。开发时可在建表后执行一次：

```sh
sqlite3 -bail acorn.db < examples/seed-user.sql
```

[seed-user.sql](../examples/seed-user.sql) 写入用户 1，保存合法 Argon2id PHC 哈希（19 MiB、2 次迭代、并行度 1）。哈希由已丢弃的随机密码生成，脚本不提供登录凭据，当前也不启用登录功能。此文件用于开发；正式部署自行预置用户。已发布示例题后续需要修改时，按题目版本规则发布新版本。

API 与 Scheduler 每次打开数据库连接后，都必须在事务开始前执行 `PRAGMA foreign_keys = ON`；此设置属于连接，不会随数据库文件保存，见 [SQLite 外键文档](https://www.sqlite.org/foreignkeys.html)。源码、输入和期望输出按 BLOB 绑定，SQL 参数使用驱动提供的参数绑定。

数据库约束负责字段类型、唯一性、引用关系、枚举、正值及终态字段的空值关系。UUID 格式、密码哈希、最新版本选择、发布后不可变、连续用例序号、配置中的大小与限额上限、具体结果形状及状态转换条件由应用实现。保存终态仍需使用带原状态条件的更新，防止迟到结果覆盖已有终态。

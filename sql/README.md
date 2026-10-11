# 数据库 SQL

[001-initial-schema.sql](001-initial-schema.sql) 实现[数据库设计版本 1](../docs/02-database-design.md)的六张业务表、外键、约束和索引。数据库结构版本记录在 `PRAGMA user_version` 中。

使用 SQLite 3.38.0 或更新版本，并启用 JSON 函数。表使用 [STRICT 模式](https://www.sqlite.org/stricttables.html)约束存储类型；[JSON 函数](https://www.sqlite.org/json1.html)检查终态详情是合法 JSON 对象。

在仓库根目录对新的空数据库执行一次：

```sh
sqlite3 -bail acorn.db < sql/001-initial-schema.sql
```

脚本自行开启并提交事务。应用使用驱动执行时，应在尚未开始事务的连接上运行整个脚本；发生错误时回滚并报告，不继续使用未初始化完成的数据库。脚本不插入业务数据。

[002-example-problem.sql](002-example-problem.sql) 提供随正式发布保留的内置题目“两数求和（A + B）”。在新库建表后执行一次：

```sh
sqlite3 -bail acorn.db < sql/002-example-problem.sql
```

该文件是初始业务数据，数据库结构版本仍为 1。它写入题号 1、版本 1，允许 C 和 Python，包含四个按顺序执行的用例；输入和答案均为 BLOB。C 的单用例预算为 CPU 1 秒、墙钟 3 秒、内存 256 MiB；Python 为 CPU 2 秒、墙钟 5 秒、内存 256 MiB。发布在一个事务中完成：先创建版本指针为空的题目，再写版本、语言和用例，最后指向版本 1。

示例脚本用于新库，不覆盖已有题目。它不创建用户；首版测试用户仍由部署预置，后端配置其 `user_id`。已发布示例题后续需要修改时，按题目版本规则发布新版本。

API 与 Scheduler 每次打开数据库连接后，都必须在事务开始前执行 `PRAGMA foreign_keys = ON`；此设置属于连接，不会随数据库文件保存，见 [SQLite 外键文档](https://www.sqlite.org/foreignkeys.html)。源码、输入和期望输出按 BLOB 绑定，SQL 参数使用驱动提供的参数绑定。

数据库约束负责字段类型、唯一性、引用关系、枚举、正值及终态字段的空值关系。UUID 格式、密码哈希、最新版本选择、发布后不可变、连续用例序号、配置中的大小与限额上限、具体结果形状及状态转换条件由应用实现。保存终态仍需使用带原状态条件的更新，防止迟到结果覆盖已有终态。

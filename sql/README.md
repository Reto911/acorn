# 数据库 SQL

本目录提供 MySQL 8.4／InnoDB 的全新初始化脚本，结构版本为 4。后端使用已初始化的数据库，启动时检查结构版本和配置用户，不建表或自动升级。数据合同见[数据库设计](../docs/02-database-design.md)，连接与事务合同见[MySQL 迁移设计](../docs/09-mysql-migration-design.md)。

## 脚本与预置内容

从仓库根目录按以下顺序执行：

| 顺序 | 文件 | 内容 |
| --- | --- | --- |
| 1 | [001-initial-schema.sql](001-initial-schema.sql) | 六张业务表、结构版本表、外键、CHECK 和索引；全部 DDL 成功后写入 `(id=1, version=4)` |
| 2 | [seed-user.sql](../examples/seed-user.sql) | 开发用户 1，用户名 test、student 角色、active 状态；密码哈希不提供可用登录凭据，当前应用没有登录功能 |
| 3 | [002-example-problem.sql](002-example-problem.sql) | 随发布保留的题号 1、版本 1 两数求和题，C／Python 限额与 104 个有序用例；默认关闭自动 LLM 审查 |

题目脚本保留四个原始用例、20 个边界用例及 80 个固定随机种子生成的用例，输入和期望输出使用二进制字面量保存。它只装入题目数据，不创建用户或改变结构版本。

开发用户是 `users` 表中的业务记录；用于客户端和后端连接的 MySQL 账户由部署端另行准备。

## 账户与数据库准备

先由部署管理员创建空数据库，例如：

```sql
CREATE DATABASE appdb CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin;
```

账户职责如下：

| 用途 | 权限与职责 |
| --- | --- |
| 部署管理员 | 准备数据库和账户，处理初始化失败后的新库重建 |
| 初始化账户 | 在目标库执行建表、外键及索引 DDL 和预置数据 DML；不作为后端运行账户使用 |
| 后端运行账户 | 读取业务数据，插入提交并更新判题及审查状态；不承担建库、建表、升级或删库职责 |
| 开发测试账户 | 在专用前缀下创建、初始化及删除独立测试库，故障注入另需 TRIGGER、LOCK TABLES 及服务器允许创建触发器的环境，见[后端测试说明](../crates/backend/README.md#检查命令) |

连接文本使用 utf8mb4／utf8mb4_0900_bin。三个脚本会为各自会话设置字符集和严格 SQL 模式，不修改服务端全局设置。

## 初始化

以下示例使用初始化账户，每条命令提示输入该账户密码。当前 MySQL 连接明确禁用 SSL：

```sh
mysql --host=127.0.0.1 --port=3306 --user=example_initializer --password --ssl-mode=DISABLED --database=appdb < sql/001-initial-schema.sql
mysql --host=127.0.0.1 --port=3306 --user=example_initializer --password --ssl-mode=DISABLED --database=appdb < examples/seed-user.sql
mysql --host=127.0.0.1 --port=3306 --user=example_initializer --password --ssl-mode=DISABLED --database=appdb < sql/002-example-problem.sql
```

目标库必须为空，三个脚本各执行一次。MySQL 客户端在首个错误时停止，不使用 `--force`。常规 DDL 会隐式提交，初始结构脚本不能整体事务回滚；若初始化失败，由部署端处理这次新库并从空库重新执行，不在半成品结构上继续初始化。用户和题目预置分别使用 DML 事务。

初始化完成后，后端 TOML 中的 `database.endpoint`、`database.name`、`database.username` 指向该库及后端运行账户，`test_user.user_id` 对应预置用户 1。后端运行账户密码通过进程环境变量 `MYSQL_PASSWORD` 提供，正式程序不自动读取 `.env`。完整配置见[后端说明](../crates/backend/README.md#配置)，程序启动和公开样例提交见[Quick Start](../README_zh-CN.md#quick-start)。

后续启动复用已初始化的库，不重跑这些脚本。本套脚本不导入旧 SQLite 数据，也不提供已有库的在线升级入口。

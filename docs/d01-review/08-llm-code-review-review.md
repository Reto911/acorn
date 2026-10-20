# LLM 代码审查方案记录

对应设计：[LLM 代码审查设计](../08-llm-code-review-design.md)。本记录保留 SQLite 阶段的数据库交付约定与原任务定义；当前初始化合同以[MySQL 迁移设计](../09-mysql-migration-design.md)为准，实际交付和检查见[审查实施记录](../d02-execution/08-llm-code-review-execution.md#121-l01数据契约与离线升级)。

## 1. 历史数据库交付约定

本节描述 L01 当时的 SQLite 版本 2 → 3 交付。当前仓库中的 `sql/001-initial-schema.sql` 已按[MySQL 迁移设计](../09-mysql-migration-design.md)提供版本 4 全新初始化结构，不用于本节的旧库升级。

已有库的版本 2 → 3 升级 SQL 和执行命令由实现方单独提供，不生成或保留仓库内的迁移脚本。交付前在临时数据库核对升级 SQL、旧记录默认值及查询；新库结构和升级后的结构应一致。

实际升级由部署方停服务、备份后，通过 `sqlite3 -bail` 执行单独提供的升级 SQL，操作步骤见[第 12.1.2 节](../d02-execution/08-llm-code-review-execution.md#1212-离线升级交付)。应用启动只检查版本，不自动升级；旧提交默认 DISABLED，不自动补跑审查，也不对现有库重跑完整初始化 SQL。

## 2. 原 L01 任务定义

1. 在后端定义 ReviewMode、ReviewStatus、最新一次详情及意见类型。固定五个状态、三个模式、闭区间行号、result/error/诊断对象的关系及[原设计第 5.2 节](../08-llm-code-review-design.md#52-最近一次详情)的内容预算；prompt_version 仅后端使用，不保留独立 Schema 版本。
2. 仓库新库 SQL 增加四列并提供完整版本 3 结构；单独交付一次性的版本 2 → 3 升级 SQL 和执行命令，不将迁移脚本放入仓库。保留既有提交和判题终态，新增列采用默认值；应用结构版本检查同步更新，不实现自动迁移。
3. 增加读取固定版本 review_mode、最新审查详情、按 review_id 条件完成，以及中断旧 PENDING 的明确数据库操作。扩展机器终态保存以原子写入自动审查初态，保留更新是否生效的返回值。
4. 扩展历史查询的 review_status 可选条件，与既有筛选组合。不为每个字段新增仓储层或索引。
5. 复用临时 SQLite 夹具核对新库及一次旧库升级、默认 DISABLED、最新一次详情往返及[原设计第 5.2 节](../08-llm-code-review-design.md#52-最近一次详情)写入预算；条件写入及机器终态保留复用现有测试补必要断言，不另建数据库故障矩阵。

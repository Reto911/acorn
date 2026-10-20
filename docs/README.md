# 设计文档

本索引介绍各设计与开发计划的目的和主要内容。审核讨论和执行过程分别见 [审核记录目录](d01-review/)与[实施记录目录](d02-execution/)。

后端工程构建、检查命令与程序入口见[项目说明](../README_zh-CN.md)，浏览器工程安装、代理、检查与静态部署见[前端说明](../web/README.md)。

- [Worker 设计](01-worker-design.md)：推送式 gRPC、串行判题、C/Python 语言适配器、沙箱隔离、结果保留与英文日志。
- [数据库设计](02-database-design.md)：持久化数据模型、题目版本与外键、提交历史和判题结论、幂等事务、审查存储及结果确认。
- [数据库 SQL](../sql/README.md)：初始建表脚本、正式保留的两数求和示例题与执行说明。
- [后端与 Scheduler 设计](03-backend-design.md)：后端组件职责、进程内任务队列、Worker gRPC 客户端、HTTP 契约、数据库访问与启动停机流程。
- [Worker 与后端开发计划](04-development-plan.md)：十个开发步骤、工程产物、前置依赖、完成条件与首版交付检查。
- [前端 GUI 设计](05-frontend-design.md)：前端技术选型、题目与答题页面、判题及审查结果、提交历史、本地草稿、HTTP 接入和健康观测边界。
- [前端开发计划](06-frontend-development-plan.md)：GUI 与健康观测的任务划分、接口依赖、实施顺序和验收要求。
- [健康观测设计与实施计划](07-health-observation-design.md)：Worker 状态 RPC、后端共享采集、只读 HTTP、健康页面的刷新与过期语义，以及实施和验收要求。
- [LLM 代码审查设计](08-llm-code-review-design.md)：Responses API 接入、自动及手动审查、最新意见持久化、独立队列、资源与生命周期边界、页面交互及验收要求。
- [MySQL 迁移设计与执行方案](09-mysql-migration-design.md)：迁移范围与设计依据、全新初始化、配置与连接池、存储和事务合同、错误处理、任务划分及验收要求。

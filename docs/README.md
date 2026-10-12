# 设计文档

后端工程构建、检查命令与当前程序入口见[项目说明](../README.md)，浏览器工程安装、代理、检查与静态部署见[前端说明](../web/README.md)。

- [Worker 设计](01-worker-design.md)：推送式 gRPC、串行判题、C/Python 语言适配器、沙箱隔离、结果保留与英文日志。
- [数据库设计](02-database-design.md)：统一 SQLite 数据库、题目版本与外键、提交历史和判题结论、内存调度队列与结果确认。
- [数据库 SQL](../sql/README.md)：初始建表脚本、正式保留的两数求和示例题与执行说明。
- [后端与 Scheduler 设计](03-backend-design.md)：Axum/Tokio/SQLx 后端、进程内任务队列、Worker gRPC 客户端、HTTP 契约与启动停机流程。
- [Worker 与后端开发计划](04-development-plan.md)：十个开发步骤、工程产物、前置依赖、完成条件与首版交付检查。
- [前端 GUI 设计](05-frontend-design.md)：Vue 3/TypeScript/Monaco 选型、题目与答题、提交结果和历史、本地草稿、HTTP 接入及健康观测边界。
- [前端开发计划](06-frontend-development-plan.md)：F01–F07 首版 GUI 任务、接口依赖与验收，F08 后续健康观测。
- [健康观测设计与实施计划](07-health-observation-design.md)：已确认的 F08 契约，H01 Worker 状态 RPC 已实现；包含待接入的只读 HTTP、共享探测、过期警告、实施步骤与有限验收。

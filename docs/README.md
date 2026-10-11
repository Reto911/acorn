# 设计文档

- [Worker 设计](01-worker-design.md)：推送式 gRPC、串行判题、C/Python 语言适配器、沙箱隔离、结果保留与英文日志。
- [数据库设计](02-database-design.md)：统一 SQLite 数据库、题目版本与外键、提交历史和判题结论、内存调度队列与结果确认。
- [数据库 SQL](../sql/README.md)：初始建表脚本与执行说明。
- [后端与 Scheduler 设计](03-backend-design.md)：Axum/Tokio/SQLx 后端、进程内任务队列、Worker gRPC 客户端、HTTP 契约与启动停机流程。

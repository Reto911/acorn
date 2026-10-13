# 设计文档

后端工程构建、检查命令与当前程序入口见[项目说明](../README.md)，浏览器工程安装、代理、检查与静态部署见[前端说明](../web/README.md)。

LLM 增量合同以[LLM 代码审查设计](08-llm-code-review-design.md)为依据，并同步维护数据库、后端和前端设计中的相关合同。L01 已交付数据类型、结构版本 3 及数据库基础操作，L02 已交付配置、固定策略和单次 Responses 客户端，L03 已交付独立队列、自动交接及启动停机；手动 HTTP、查询字段和 GUI 仍待 L04–L06。

- [Worker 设计](01-worker-design.md)：推送式 gRPC、串行判题、C/Python 语言适配器、沙箱隔离、结果保留与英文日志。
- [数据库设计](02-database-design.md)：统一 SQLite 数据库、题目版本与外键、提交历史和判题结论、内存调度队列与结果确认；已实现 L01 的 LLM 字段、最新审查基础读写和结构版本 3，保留离线升级合同。
- [数据库 SQL](../sql/README.md)：初始建表脚本、正式保留的两数求和示例题与执行说明。
- [后端与 Scheduler 设计](03-backend-design.md)：Axum/Tokio/SQLx 后端、进程内任务队列、Worker gRPC 客户端、HTTP 契约与启动停机流程；L03 已接入独立审查队列、自动触发和生命周期，手动接口与审查查询字段待 L04。
- [Worker 与后端开发计划](04-development-plan.md)：十个开发步骤、工程产物、前置依赖、完成条件与首版交付检查。
- [前端 GUI 设计](05-frontend-design.md)：Vue 3/TypeScript/Monaco 选型、题目与答题、提交结果和历史、本地草稿、HTTP 接入及健康观测边界；同步待实施的审查结果区、手动触发、联合轮询及历史筛选。
- [前端开发计划](06-frontend-development-plan.md)：F01–F07 首版 GUI 任务、接口依赖与验收，F08 健康观测及初次连通验收记录，后续每 10 秒自动获取的实现与验证边界。
- [健康观测设计与实施计划](07-health-observation-design.md)：已确认的 F08 契约，H01–H04 已实现 Worker 状态 RPC、后端共享采集、只读 HTTP、支持自动获取及手动刷新的健康页面和独立过期警告；包含实施步骤与有限验收。
- [LLM 代码审查设计](08-llm-code-review-design.md)：L01–L03 已完成，L04–L06 待实施；定义 Responses API 接入、判题后自动门槛及手动审查、最新意见持久化、后端独立队列、提交页面 UI 和实施验收步骤。

# Acorn 前端

独立的 Vue 3 + TypeScript 浏览器工程。当前完成 F01–F02：中文浅色页面骨架、路由懒加载、本地 Monaco 和编辑器 Worker，以及 HTTP 客户端、查询生命周期、中文错误、浏览器日志和待完成提交请求的恢复/重试/放弃入口。编辑器暂为只读；题目展示、主动提交、草稿与结果轮询分别在 F03–F06 接入，页面不使用模拟业务数据。

设计和任务依据见[前端 GUI 设计](../docs/05-frontend-design.md)和[前端开发计划](../docs/06-frontend-development-plan.md)。

## 环境与安装

- Node.js：24.x，至少 24.21.0；本任务使用 24.21.0，`.nvmrc` 固定该版本。
- pnpm：12.11.2，`package.json` 的 `packageManager` 固定版本。
- Vue 3.5.43、Vue Router 4.6.4、Element Plus 2.14.7、Monaco 0.57.0、Vite 8.3.4、TypeScript 6.0.3、Vitest 5.0.4；完整依赖版本见 `package.json` 和 `pnpm-lock.yaml`。

在本目录执行：

```sh
nvm use
corepack enable
pnpm install --frozen-lockfile
cp .env.example .env.local
pnpm dev
```

`nvm use` 仅适用于使用 nvm 的环境；其他安装方式也可提供上述 Node.js 版本。若已有 pnpm 12.11.2，无需重复启用 Corepack。项目只维护 pnpm 锁文件，不混用 npm/yarn 安装。

开发地址为 `http://localhost:5173`，默认只监听本机 `127.0.0.1`。端口占用时明确退出，不自动改用其他端口。开发期间按[后端启动说明](../README.md#程序入口)启动后端，使用已初始化并预置测试用户和题目的数据库；F01 只读代理检查无需 Worker。不要重复初始化已有数据库。

## 配置与 API 代理

| 变量                    | 默认值                  | 用途                                           |
| ----------------------- | ----------------------- | ---------------------------------------------- |
| `API_PROXY_TARGET`      | `http://127.0.0.1:8080` | Vite 服务端读取的后端地址，不注入浏览器        |
| `VITE_SOURCE_MAX_BYTES` | `65536`                 | 浏览器源码 UTF-8 字节上限；供 F04 提交预检使用 |

在 `.env.local` 修改开发配置后重启 Vite。前端 HTTP 客户端统一使用相对 `/api/v1` 路径，不直接写后端主机地址。代理将 `/api` 原样转发，因此 `/api/v1/problems` 在后端仍为 `/api/v1/problems`，无需后端新增 CORS。

后端运行时，在另一终端检查代理：

```sh
curl --fail-with-body http://localhost:5173/api/v1/problems
```

应取得真实后端的 `items`、`limit`、`offset` 分页 JSON。题目列表界面在 F03 接入；目前页面骨架可用与 API 连通分别检查。此请求只证明题目 API 可访问，不代表 Worker 健康。

`VITE_` 变量属于公开构建配置，不能放入秘密。源码上限需与后端 `[request].source_max_bytes` 对齐；生产配置必须在构建时提供，部署后修改 `.env.local` 不会改变已有产物。后端始终执行最终校验。

## 检查命令

在本目录执行：

```sh
pnpm format:check
pnpm lint
pnpm typecheck
pnpm build
pnpm test
```

`pnpm format` 可格式化前端文件；锁文件由 pnpm 生成，不手工格式化。`pnpm build` 包含类型检查，产物为 `dist/`。`pnpm test` 单次运行 Vitest，`pnpm test:watch` 用于开发期间监听。F02 已有 4 个 `src/**/*.test.ts` 文件、36 项 HTTP、请求快照、查询生命周期与日志测试，不再允许空测试集作为成功检查；草稿和结果轮询测试随对应任务加入。

纯前端变更使用这些检查及相关验证，不重复运行 Rust/Isolate 验收；如果修改 Rust，仍遵守仓库规定的格式、编译、Clippy 和相关测试要求。

## HTTP 与待完成的提交请求

`src/api/types.ts` 对应后端 HTTP JSON，`src/api/index.ts` 提供题目列表、最新/固定版本题面、创建、详情和历史调用。请求使用 10 秒超时和可选 `AbortSignal`，HTTP 错误、网络、超时、取消及无法解析响应由 `RequestError` 区分，业务 `failed` 仍是成功取得的提交详情。`useQuery` 接受页面身份/查询参数 getter，在变化和离页时取消并忽略旧响应；`reload()` 保留最近成功数据，供页面显示过期提示和手动重试。

主动提交接线时使用 `useSubmissionRequest().start(body)`：它生成一次 UUID，复制并冻结 `problem_id`、`language_id`、`source`，在发送前写入当前标签页 `sessionStorage`。待完成请求存在时不允许开始另一份请求；`retry()` 只重传原快照和原键，不读取编辑器或草稿。成功和明确业务拒绝清除快照，队列满（包括已有 ID）、后端停止接纳和响应不明时保留。底层 `createSubmission(requestKey, snapshot)` 不生成新键、不自动重试。

答题页已经提供待完成请求的恢复提示、手动重试和确认放弃。刷新不自动 POST；队列满带 ID 时提供记录链接，成功重试才导航结果页。请求在途不能放弃，确认放弃不会撤销后端提交，也不会清空草稿。会话存储不可用时明确提示本次请求只能在内存中保留，刷新可能失去原请求的恢复入口。长期 `localStorage` 草稿和从编辑器发起新提交仍在 F04 接入。

`RequestFeedback.vue` 提供加载、空数据、可操作中文报错与手动重试。公共日志只在浏览器控制台输出请求元数据、创建结果、状态变化及异常类型/调用栈帧，不输出源码、请求正文、幂等键或程序输出；生产默认关闭 debug。未捕获异常另显示页面提示，不把 HTTP 错误当作判题终态，也不据此宣称 Worker 健康。

## 路由、组件与按需加载

| 路径                         | 页面             |
| ---------------------------- | ---------------- |
| `/`                          | 跳转 `/problems` |
| `/problems`                  | 题目列表         |
| `/problems/:problemId`       | 答题             |
| `/submissions`               | 提交历史         |
| `/submissions/:submissionId` | 提交结果         |
| 其他路径                     | 前端 404         |

`src/router.ts` 使用 HTML5 history，页面均通过动态导入加载。Element Plus 组件和对应样式在使用处显式导入，中文 locale 由根组件提供，不进行全量组件注册。答题页再异步加载 `CodeEditor.vue`，其中引入 Monaco 的 ESM 编辑器与 `?worker` 资源；题目列表、提交历史和结果页不会加载 Monaco。资源来自本地构建产物，不依赖运行时 CDN。C/Python 模式、编辑模型与语言切换在 F04 接入。

## 构建与静态部署

```sh
pnpm build
pnpm preview --host 127.0.0.1 --port 4173
```

`preview` 仅用于本地预览构建产物。实际部署由静态服务器提供 `dist/`，同源 `/api/` 反向代理到 Axum；后端仍只提供 API。当前工程按域名根路径部署。

Nginx 路由示例（路径和监听地址按实际部署修改）：

```nginx
server {
    listen 80;
    server_name localhost;
    root /srv/acorn/web/dist;
    index index.html;

    location = /api {
        proxy_pass http://127.0.0.1:8080;
    }

    location ^~ /api/ {
        proxy_pass http://127.0.0.1:8080;
    }

    location ^~ /assets/ {
        try_files $uri =404;
    }

    location ~ \.[^/]+$ {
        try_files $uri =404;
    }

    location / {
        try_files $uri $uri/ /index.html;
    }
}
```

页面路径如 `/submissions/<id>` 必须回退到 `index.html`，才能直接打开和刷新。API 错误由后端返回，不回退成 HTML；不存在的静态资源返回 404。Monaco Worker 必须作为同源构建资源提供。

后续创建提交使用 `crypto.randomUUID()`：本地开发使用 localhost，远程部署配置 HTTPS。上述 HTTP 示例仅说明静态文件与 API 路由，不包含 TLS 配置。当前没有登录或健康观测页面。

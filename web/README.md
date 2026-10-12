# Acorn 前端

独立的 Vue 3 + TypeScript 浏览器工程。当前完成 F01：中文浅色布局、题目/答题/提交历史/结果/404 页面骨架、路由懒加载，以及只在答题页加载的本地 Monaco 和编辑器 Worker。编辑器暂为只读；真实题目、HTTP 客户端、提交、草稿与结果轮询分别在 F02–F06 接入，页面不使用模拟业务数据。

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

在 `.env.local` 修改开发配置后重启 Vite。前端 HTTP 客户端从 F02 开始统一使用相对 `/api/v1` 路径，不直接写后端主机地址。代理将 `/api` 原样转发，因此 `/api/v1/problems` 在后端仍为 `/api/v1/problems`，无需后端新增 CORS。

后端运行时，在另一终端检查代理：

```sh
curl --fail-with-body http://localhost:5173/api/v1/problems
```

应取得真实后端的 `items`、`limit`、`offset` 分页 JSON。F01 页面尚不请求该列表；页面骨架可用与代理连通分别检查。此请求只证明题目 API 可访问，不代表 Worker 健康。

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

`pnpm format` 可格式化前端文件；锁文件由 pnpm 生成，不手工格式化。`pnpm build` 包含类型检查，产物为 `dist/`。`pnpm test` 单次运行 Vitest，`pnpm test:watch` 用于开发期间监听；F01 没有业务测试，暂用 `--passWithNoTests` 允许空测试集退出成功，这不表示已执行测试。后续针对幂等请求、草稿和轮询等契约增加 `src/**/*.test.ts`，再按需要配置组件测试环境。

纯前端变更使用这些检查及相关验证，不重复运行 Rust/Isolate 验收；如果修改 Rust，仍遵守仓库规定的格式、编译、Clippy 和相关测试要求。

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

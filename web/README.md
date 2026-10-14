# Acorn Web

Acorn 的浏览器客户端，使用 Vue 3、TypeScript、Element Plus、Monaco 和 Vite。提供中文界面，面向宽度不小于 1024 CSS px 的桌面浏览器。

## 功能与页面

| 路径                         | 功能                                            |
| ---------------------------- | ----------------------------------------------- |
| `/problems`                  | 分页浏览已发布题目                              |
| `/problems/:problemId`       | 阅读题面、选择语言、编辑代码、保存草稿并提交    |
| `/submissions`               | 按题目、执行状态、判题结论和审查状态筛选历史    |
| `/submissions/:submissionId` | 查看机器结果、诊断、固定版本题面和 LLM 审查意见 |
| `/health`                    | 查看后端接纳条件、队列、数据库和 Worker 状态    |

访问 `/` 会跳转题目列表。页面按路由加载，答题页再加载 Monaco；编辑器及其 Worker 资源随工程构建并从同源提供。

## 环境与安装

- Node.js 24.x，至少 24.21.0；[.nvmrc](.nvmrc)指定 24.21.0。
- pnpm 12.11.2；[package.json](package.json)固定包管理器版本。
- 按[根说明](../README_zh-CN.md#quick-start)启动后端和 Worker，准备数据库、用户与题目。

安装 pnpm：

```sh
npm install --global pnpm@12.11.2
```

在 `web/` 目录安装依赖并启动：

```sh
pnpm install --frozen-lockfile
cp .env.example .env.local
pnpm dev
```

打开[本地题目列表](http://localhost:5173/problems)。开发服务监听 `127.0.0.1:5173`，端口占用时退出。使用 nvm 管理 Node.js 时，可在本目录通过 `nvm install` 和 `nvm use` 选择 `.nvmrc` 指定版本。

## 配置与 API 代理

| 变量                    | 默认值                  | 用途                        |
| ----------------------- | ----------------------- | --------------------------- |
| `API_PROXY_TARGET`      | `http://127.0.0.1:8080` | Vite 服务端的后端代理目标   |
| `VITE_SOURCE_MAX_BYTES` | `65536`                 | 提交前检查源码 UTF-8 字节数 |

在 `.env.local` 中修改配置后重启 Vite。浏览器使用相对 `/api/v1` 路径，开发代理原样转发 `/api`，例如：

```sh
curl --fail-with-body http://localhost:5173/api/v1/problems
```

`VITE_SOURCE_MAX_BYTES` 应与后端 `request.source_max_bytes` 对齐，在生产环境构建时注入。`VITE_` 变量会进入浏览器构建产物；模型密钥配置在后端 TOML。

## 快速使用

1. 打开两数求和题，选择 C 或 Python 3。
2. 将 [C 样例](../examples/submissions/a-plus-b.c)或 [Python 样例](../examples/submissions/a-plus-b.py)粘贴到编辑器。
3. 使用“保存草稿”或 Ctrl+S/Cmd+S 保存代码，点击“提交代码”进入结果页。
4. 等待判题完成，查看结论、编译诊断或首个失败用例；展开“判题版本题面”查看本次实际使用的版本。
5. 从“本题提交记录”查看历史，使用筛选和分页定位记录，再进入详情。

### 编辑与草稿

编辑器支持 C/Python 语法高亮、撤销、查找和行号。草稿按题目及语言保存在 `localStorage`，输入后约 1 秒自动保存，切换语言或离页时补存。重新打开答题页会恢复草稿；题面版本改变时保留源码并提示复核。

Ctrl+S/Cmd+S 保存草稿，提交由“提交代码”按钮触发。提交成功后草稿继续保留。“清空本语言草稿”只清除当前题目、当前语言的内容。草稿保存失败会在页面提示，可继续编辑和提交。

### 请求恢复

提交时冻结题号、语言与源码并生成 UUID 幂等键，将待完成请求保存在当前标签页的 `sessionStorage`。网络中断、超时或队列满后，可使用页面的重试入口重传原请求；后续编辑保存在草稿中。

刷新后会显示待完成请求的恢复入口，由用户选择重试或放弃。放弃只移除浏览器请求快照，后端已接纳的提交继续执行。

### 结果与历史

结果页在机器判题未结束或审查为 PENDING 时轮询，上一请求结束约 1 秒后再次查询。标签页隐藏时暂停，重新可见时继续；查询失败保留最近快照并提供手动重试。

结果显示 AC、WA、CE、RE、TLE、MLE、OLE 或执行故障，诊断和输出按文本呈现。资源统计换算为 ms 和 MiB，缺失值显示为未提供。历史记录关联其固定题目版本；“返回答题”打开最新题面和本地草稿。

历史筛选、页长和位置保存在 URL 中，支持刷新、前进后退及详情往返。历史列表展示后端配置用户的提交记录。

## 代码审查

结果页独立展示审查状态、模型、请求和完成时间、摘要及疑点。疑点包含类别、从 1 开始且包含首尾的源码行范围、证据和可能的正常解释。审查意见按文本显示。

启用后端 LLM 服务后，机器 `state=finished` 的提交可“启动审查”，包括 CE 和首例失败；已有结果可“重新审查”，接纳后替换上次意见。自动触发取决于固定题目版本的策略。配置方法见[后端说明](../crates/backend/README.md#llm-代码审查)。

审查状态为未启用／未触发、审查中、通过、存疑或审查失败。手动操作后页面立即查询最新状态并继续轮询；请求结果不明时先查询确认。历史页可按审查状态筛选。

## 健康观测

“系统状态”页面进入时查询一次，此后每 10 秒获取快照，也可点击“刷新状态”。显示内容包括提交接纳条件、Scheduler 名额、数据库读取及各 Worker 的执行占用、停机标记和未确认结果容量。

请求在途时跳过周期获取；请求失败保留上次快照，下个周期继续。过期观测单独显示警告及各自检查时间，成功刷新后逐项更新。离页时清理定时器并取消浏览器等待。

页面通过 `GET /api/v1/health` 读取后端缓存。观测状态和字段含义见[健康观测设计](../docs/07-health-observation-design.md#5-http-契约与可见范围)。

## 构建与静态部署

在本目录执行：

```sh
pnpm build
pnpm preview --host 127.0.0.1 --port 4173
```

`build` 先执行 TypeScript 类型检查，再生成 `dist/`；`preview` 用于本地预览。部署时由静态服务器提供 `dist/`，将同源 `/api` 和 `/api/` 原样代理到后端。当前工程使用域名根路径。

Nginx 路由示例，按部署修改域名、监听地址和静态目录：

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

页面路径回退到 `index.html`，支持结果链接直接打开和刷新；API 响应保留后端状态码，缺失静态资源返回 404。远程浏览器通过 HTTPS 访问，以支持创建提交使用的 `crypto.randomUUID()`。

## 源码结构

| 目录或文件         | 内容                                     |
| ------------------ | ---------------------------------------- |
| `src/pages/`       | 题目、答题、结果、历史和健康页面         |
| `src/components/`  | 编辑器、题面、诊断、审查和请求反馈组件   |
| `src/api/`         | HTTP 类型、请求与错误处理                |
| `src/composables/` | 查询、轮询、草稿及提交／审查请求生命周期 |
| `src/utils/`       | 状态展示、历史参数、时间及日志工具       |
| `src/router.ts`    | 页面路由和按需加载                       |
| `vite.config.ts`   | 开发监听和 API 代理                      |

## 检查命令

```sh
pnpm format:check
pnpm lint
pnpm typecheck
pnpm build
pnpm test
```

`pnpm format` 格式化源码与文档，`pnpm test:watch` 以监听模式运行测试。依赖版本由 [pnpm-lock.yaml](pnpm-lock.yaml)锁定。界面和交互合同见[前端 GUI 设计](../docs/05-frontend-design.md)。

# Spec: API 密钥能力增强与 Pandar CLI 客户端（`pandar login` / `pandar client`）

## 状态与基线

- 状态：implemented（已通过 `astra-review` 规格与代码审查）
- Pandar 基线：`32cede61` on `main`（2026-11-28）
- 关联文档：
  - `contracts/hub-client.openapi.json`
  - `docs/roadmap.md`

## 背景与问题

Pandar Hub 已经具备租户级 Bearer Token 体系（`tenant_tokens`）以及管理员后台的创建、轮换、吊销界面，几乎所有 `/api/v1/tenants/{tenant_id}/...` 接口都已支持 `Authorization: Bearer <token>`。但在将它作为通用 **API 密钥** 与 **命令行客户端** 使用时，存在四个明确缺口：

1. **权限粒度断层**：当前可手动签发的 scope 只有 `*`（等同 `TenantAdmin`）、空数组 `[]`（只读 `Viewer`）、`agent:register` 和 `plugin:studio`，缺少面向日常自动化打印与控制的 `operator` 权限，也缺少语义明确的 `viewer` scope。
2. **缺少凭据自省接口**：`GET /api/v1/me` 只接受外部身份提供商的 JWT，不接受 Tenant Token。外部脚本或 CLI 拿到一把 API 密钥后，无法验证密钥有效性，也无法自动得知其归属的 `tenant_id`、租户名称、scope 与有效角色。
3. **缺少 CLI 浏览器登录链路**：虽然 Hub 和 Web 已经为 Studio 插件、Android 和 Web 应用实现了基于一次性票据（`plugin_login_tickets`）+ PKCE S256 的登录流，但尚未提供面向本机 CLI 回环回调（`http://127.0.0.1:<port>/callback`）的签发与换取接口，也没有对应的 Web 确认页。
4. **CLI 缺少客户端命令**：`crates/pandar-app` 提供的 `pandar` 二进制目前只包含 `hub`、`agent`、`cleanup`、插件安装等运维子命令，尚未支持 `pandar login`、`pandar logout` 与 `pandar client`。

## 目标

复用并扩展现有 `tenant_tokens` 与登录票据基础设施，提供完整的 API 密钥交互体验与 `pandar` CLI 客户端：

1. 扩展 `TenantTokenScope`，支持手动签发 `operator` 与 `viewer` API 密钥，以及通过浏览器 PKCE 登录签发 `cli:session` 会话令牌；
2. 提供 `GET /api/v1/auth/whoami` 凭据自省接口，绕过 no-auth 匿名旁路，始终严格校验 Bearer Tenant Token；
3. 提供完整的 CLI 浏览器登录与会话注销链路（`/cli-sign-in` + PKCE S256 + 严格回环回调 + 幂等远端会话自吊销）；
4. 优化 Web 控制台的租户令牌/API 密钥创建与展示体验（受控输入、快捷权限预设与新 scope 中英文徽章）；
5. 在 `pandar` CLI 中新增 `pandar login`、`pandar logout` 和 `pandar client`（同时提供受保护的通用 `request` 请求能力与常用资源结构化子命令）；
6. 不修改数据库表结构（零 migration），并在 SQLite 与 PostgreSQL 上验证并发事务、回滚及旧数据兼容等价性。

## 非目标

- **不新增用户级 Personal API Key（PAT）**：本期复用租户管理员管理的 `tenant_tokens` 作为 API 密钥；普通用户（含 Operator/Viewer）通过浏览器 `pandar login` 获取跟随其当前角色的 `cli:session` 令牌即可使用 CLI。
- **不引入跨租户超级密钥**：每把 API 密钥和每个 CLI 会话都严格绑定单个租户。
- **不做交互式 TUI 或实时事件流客户端**：首期 `pandar client` 聚焦 HTTP REST 请求与打印文件上传，不包含 `/printer-events` WebSocket 订阅或相机流拉取。
- **不改变既有信任边界**：Tenant Token（包括 `*` 与 `cli:session`）继续禁止调用仅限外部用户会话的管理接口。
- **不取代 `pandar-agent` 或 Bambu Studio 插件认证流程**。

## 领域与权限模型

### 1. `TenantTokenScope` 扩展与规范化

沿用现有 `tenant_tokens.scopes_json`（JSON 字符串数组），无数据库 schema 变更：

| Scope 字符串 | 枚举值 | 允许手动创建 | 等效角色 / 权限 | 前缀 | 说明 |
| --- | --- | --- | --- | --- | --- |
| `*` | `All` | 是 | `TenantAdmin`（含 Operator / Viewer） | `pandar_tenant_` | 租户全权 API 密钥（既有） |
| `operator` | `Operator` | 是 | `Operator`（含 Viewer） | `pandar_tenant_` | 新增：可操作打印机、提交/重试/重打任务、刷新/诊断，不可管理用户/令牌或删除资源 |
| `viewer` | `Viewer` | 是 | `Viewer` | `pandar_tenant_` | 新增：显式只读 API 密钥，权限与空 scopes `[]` 等价 |
| `[]`（空） | 无 | 是 | `Viewer` | `pandar_tenant_` | 既有只读语义，保持兼容 |
| `agent:register` | `AgentRegister` | 是 | 仅 Agent 配对与凭据管理 | `pandar_tenant_` | 既有，保持隔离 |
| `plugin:studio` | `PluginStudio` | 是 | 仅 Studio 插件端点 | `pandar_plugin_` / `pandar_tenant_` | 既有，保持隔离 |
| `mobile:session` | `MobileSession` | 否 | 跟随签发用户当前角色 | `pandar_mobile_` | 既有浏览器票据会话 |
| `web:session` | `WebSession` | 否 | 跟随签发用户当前角色 | `pandar_web_` | 既有浏览器票据会话 |
| `cli:session` | `CliSession` | 否 | 跟随签发用户当前角色 | `pandar_cli_` | 新增：CLI 浏览器登录会话（30 天有效） |

#### Scope 规范化规则（`POST /api/v1/tenants/{tenant_id}/tenant-tokens`）

- 对输入 `scopes` 逐项校验；包含任何未知值或 `is_user_session()` scope（`mobile:session`、`web:session`、`cli:session`）一律拒绝并返回 `400 {"error":"invalid_scope"}`；
- 按固定规范顺序去重并持久化：`*` → `operator` → `viewer` → `agent:register` → `plugin:studio`；
- 组合 scope 的有效角色取最高角色语义：
  - 含 `*`（即使同时包含 `operator`/`viewer`/`agent:register`）→ `TenantAdmin`；
  - 不含 `*` 但含 `operator`（即使同时包含 `viewer`）→ `Operator`；
  - 不含 `*`/`operator` 但含 `viewer` 或 scopes 为空 `[]` → `Viewer`；
  - 仅含 `agent:register` 与/或 `plugin:studio` → 不赋予任何通用租户角色（`effective_role = null`）；
- `agent:register` 路由继续通过 `token.has_scope(AgentRegister)`（`*` 亦满足）判定；
- `plugin:studio` 路由继续要求 `scopes == [PluginStudio]`（严格单 scope，与现有安全策略一致）。

### 2. 完整路由权限矩阵

必须同时更新 `authorize_tenant` 与 `authorize_principal_for_role` 共用的 `tenant_token_allows_role`：

```rust
fn tenant_token_allows_role(
    authenticated: &AuthenticatedTenantToken,
    required_role: UserRole,
) -> bool {
    authenticated.token.has_scope(TenantTokenScope::All)
        || (authenticated.token.has_scope(TenantTokenScope::Operator)
            && UserRole::Operator.allows(required_role))
        || (authenticated.token.has_scope(TenantTokenScope::Viewer)
            && UserRole::Viewer.allows(required_role))
        || matches!(authenticated.token.scopes.as_slice(), [scope] if scope.is_user_session())
            && authenticated
                .session_user
                .as_ref()
                .is_some_and(|user| user.role.allows(required_role))
}
```

完整资源类别授权矩阵：

| 资源类别 / 路由 | `*` | `operator` | `viewer` / `[]` | `agent:register` | `plugin:studio` | `cli:session`（随用户角色） |
| --- | --- | --- | --- | --- | --- | --- |
| 打印机只读（`GET /printers`, `/printers/{id}`, `/camera.mp4`, `/printer-events*`） | 允许 | 允许 | 允许 | 403 | 403 | Viewer+ 允许 |
| 打印机控制与更新（`PATCH /printers/{id}`, `POST /controls`, `/materials:refresh`） | 允许 | 允许 | 403 | 403 | 403 | Operator+ 允许 |
| 打印机删除（`DELETE /printers/{id}`） | 允许 | 403 | 403 | 403 | 403 | TenantAdmin 允许 |
| 任务只读（`GET /jobs`, `/jobs/{id}`） | 允许 | 允许 | 允许 | 403 | 403 | Viewer+ 允许 |
| 任务创建/预览/重试/重打/复制（`POST /printers/{id}/jobs`, `/artifact-metadata-preview`, `/retry-dispatch`, `/reprint`, `/duplicate`） | 允许 | 允许 | 403 | 403 | 403 | Operator+ 允许 |
| 任务删除/清空（`DELETE /jobs`, `/jobs/{id}`） | 允许 | 403 | 403 | 403 | 403 | TenantAdmin 允许 |
| Agent 只读（`GET /agents`）与命令查询（`GET /commands/{id}`） | 允许 | 允许 | 允许 | 403 | 403 | Viewer+ 允许 |
| Agent 打印机操作（`POST /refresh-printers`, `/discover-printers`, `/diagnose-printer`, `/link-printer`） | 允许 | 允许 | 403 | 403 | 403 | Operator+ 允许 |
| Agent 创建/删除（`POST /agents`, `DELETE /agents/{id}`） | 允许 | 403 | 403 | 403 | 403 | TenantAdmin 允许 |
| Agent 配对与凭据轮换/吊销（`POST /agent-pairings`, `/credential:rotate`, `/credential:revoke`） | 允许 | 403 | 403 | 允许 | 403 | TenantAdmin 允许 |
| 租户令牌、用户管理、审计日志（`/tenant-tokens*`, `/users*`, `/audit-events`） | 允许 | 403 | 403 | 403 | 403 | TenantAdmin 允许 |
| **仅限外部用户会话接口**（`PATCH /tenants/{id}`, `/join-links*`, `POST /*/login-tickets`） | 403 | 403 | 403 | 403 | 403 | 403 |

#### Tenant Token 授权模型之外的端点（独立鉴权 / 必须拒绝 Tenant Token）

实现与测试必须明确保留以下端点的独立信任边界，不得因本次扩展被 Tenant Token（含 `*`、`operator`、`viewer`、`cli:session`）越权访问：

| 端点类别 | 认证模型 | Tenant Token 访问行为 |
| --- | --- | --- |
| `GET/POST /api/v1/tenants`、`GET /api/v1/summary`、`POST /api/v1/bootstrap/tenant-admin` | Bootstrap Token（no-auth 模式下允许匿名） | 开启认证时返回 `401`（不是有效 Bootstrap Token） |
| `GET /api/v1/me`、`POST /api/v1/onboarding/tenants`、`POST /api/v1/join-links/accept` | 仅外部身份 JWT（`verify_external_identity`） | 返回 `401 invalid_auth_token` |
| `GET /healthz`、`GET /api/v1/auth/status`、`GET /api/v1/openapi.json`、`POST /api/v1/*/login-tickets/exchange` | 公开无认证端点 | 忽略或无需 Bearer Token |
| `/api/v1/plugin/*`（除 exchange / no-auth-session） | 严格单 scope `scopes == ["plugin:studio"]`（开启认证时还需活的 Operator+ 用户） | `*`、`operator`、`viewer`、`agent:register`、`cli:session` 等一律返回 `403 role_forbidden` |
| `GET /api/v1/agents/{agent_id}/artifacts/{artifact_id}`、`GET /api/v1/agents/{agent_id}/printers` | 仅 Agent Credential（`pandar_ac_...`） | 返回 `401` |

### 3. CLI 会话生命周期与用户变更语义

- **用户角色降级 / 升级**：每次请求鉴权通过 `authenticate_tenant_token` 实时加载 `users` 表中的 `session_user`，`cli:session` 立即按新角色生效（例如从 `Operator` 降级为 `Viewer` 后，所有写操作立刻返回 `403 role_forbidden`，`whoami` 返回 `effective_role: "viewer"`）。
- **用户被删除 / 移出租户**：`authenticate_tenant_token` 在查不到 `created_by_user_id` 对应的租户用户时返回 `Ok(None)`，**所有路由（包括 `GET /api/v1/auth/whoami`）统一返回 `401 {"error":"invalid_auth_token"}`**，绝不返回 `200` + `user: null`。
- **会话主动吊销或过期**：`authenticate_tenant_token` 过滤 `revoked_at IS NULL` 并校验 `expires_at > now`，过期或吊销后在所有业务路由与 `whoami` 均返回 `401 {"error":"invalid_auth_token"}`。

### 4. No-auth 模式语义

- **`GET /api/v1/auth/whoami` 与 `DELETE /api/v1/cli/session` 必须绕过 no-auth 匿名旁路**：二者使用专用的 Bearer Tenant Token 提取与认证路径，绝不调用会在 no-auth 下返回 `AuthenticatedPrincipal::NoAuth` 的 `authorize_principal`。即使 Hub 开启了 `no_auth`，缺少或提供无效 `Authorization: Bearer <token>` 仍必须返回 `401`。
- **No-auth 模式下禁用浏览器 CLI 票据登录**：与 `docs/specs/2026-08-12-bambu-studio-personal-data-sync.md` 保持一致，no-auth 模式不存在持久化用户主体，若签发无主（`created_by_user_id = None`）的 `cli:session` 会导致 `authenticate_tenant_token` 无法解析用户或在后续启用认证时造成权限歧义。因此：
  - `POST /api/v1/tenants/{tenant_id}/cli/login-tickets` 仅允许已认证的外部用户（`Viewer` / `Operator` / `TenantAdmin`）；
  - 当 Hub 处于 no-auth 模式（无外部用户身份）时返回 `403 {"error":"external_auth_required"}`；
  - 前端 `/cli-sign-in` 在检测到 no-auth 模式时展示清晰引导，提示本地无认证环境直接通过管理员页签发 API Key 并使用 `pandar login --server <hub_url> --api-key-stdin`。

## Hub HTTP API 契约

### 1. 专用 Bearer 鉴权路径（不走 `authorize_principal`）

在 `crates/pandar-hub/src/routes/auth.rs`（及拆分出的 `routes/auth/external.rs`）中提供独立函数：

- `authenticate_bearer_tenant_token(state, headers) -> Result<AuthenticatedTenantToken, ApiError>`：
  - 始终调用 `bearer_token(headers)?`（缺失返回 `401 missing_auth_token`，非 `Bearer ` 返回 `401 invalid_auth_token`）；
  - 仅调用 `state.auth().authenticate_tenant_token(token)`（绝不触发 `no_auth_enabled()` 放行，也绝不接受外部 JWT）；
  - 查无记录、已过期、已吊销或绑定用户已删除时返回 `401 invalid_auth_token`。
- `authorize_cli_login_ticket_creation(state, headers, tenant_id) -> Result<AuthenticatedPrincipal, ApiError>`：
  - 若 `state.no_auth_enabled()` 返回 `403 {"error":"external_auth_required"}`；
  - 否则仅允许外部身份用户且角色满足 `UserRole::Viewer` 及以上（`Viewer`、`Operator`、`TenantAdmin`），跨租户返回 `403 tenant_forbidden`，任意 `TenantToken` 返回 `403 role_forbidden`。

### 2. `GET /api/v1/auth/whoami`

挂载在 `client_api_router` 下（遵循 `PANDAR_CORS_ALLOWED_ORIGINS`）。

- 鉴权：`authenticate_bearer_tenant_token`（OpenAPI `security: [{"bearerAuth": []}]`）
- 成功响应：`200 OK`

```json
{
  "token_id": "uuid",
  "token_name": "CI deploy key",
  "tenant_id": "uuid",
  "tenant_slug": "acme",
  "tenant_display_name": "Acme Labs",
  "scopes": ["operator"],
  "effective_role": "operator",
  "expires_at": "2026-12-31T00:00:00Z",
  "user": null
}
```

- `expires_at`：`string | null`（永不过期的租户 API Key 为 `null`）；
- `effective_role`：`"tenant_admin" | "operator" | "viewer" | null`（仅含 `agent:register` / `plugin:studio` 时为 `null`）；
- `user`：`WhoamiUser | null`（会话令牌含 `{ id, email, display_name, role }`，非会话 API Key 为 `null`）；
- 错误响应：`401 {"error":"missing_auth_token" | "invalid_auth_token"}`。

### 3. `POST /api/v1/tenants/{tenant_id}/cli/login-tickets`

挂载在 `client_api_router` 下。

- 鉴权：`authorize_cli_login_ticket_creation`（OpenAPI `security: [{"bearerAuth": []}]`）
- 请求体：`{"redirect_url": "http://127.0.0.1:54321/callback", "code_challenge": "<43-char base64url>"}`
- 回调地址校验（`validate_cli_redirect_url`）：
  - 仅允许 `http` 协议；
  - host 仅允许 `localhost`、`127.0.0.1` 或 `[::1]`；
  - 必须显式携带非零端口；
  - 禁止 userinfo、query（`?`）与 fragment（`#`）；
  - 路径必须非空（如 `/callback`）。
- 票据属性：`kind = "cli"`，前缀 `pandar_cli_ticket_`，5 分钟过期，一次性使用，记录审计事件 `cli_login_ticket.create`；
- 成功响应：`201 Created` → `{"ticket": "pandar_cli_ticket_...", "expires_at": "...", "redirect_url": "..."}`；
- 错误响应：
  - `400 {"error":"bad_request" | "invalid_code_challenge" | "invalid_redirect_url"}`
  - `401 {"error":"missing_auth_token" | "invalid_auth_token"}`
  - `403 {"error":"role_forbidden" | "tenant_forbidden" | "external_auth_required"}`

### 4. `POST /api/v1/cli/login-tickets/exchange`

挂载在顶层 `router` 下（CLI 本机直连，不纳入浏览器 CORS，OpenAPI `security: []`）。

- 请求体：`{"ticket": "pandar_cli_ticket_...", "code_verifier": "<43..128-char verifier>"}`
- 事务保障：在 `self.database.begin_write_transaction()` 中原子执行：
  1. 按 `ticket_hash` + `kind = 'cli'` + `used_at IS NULL` + `revoked_at IS NULL` 加载票据；
  2. 校验未过期且 PKCE S256 匹配；
  3. 条件更新 `used_at` 并断言 `rows_affected == 1`；
  4. 创建 30 天有效 `pandar_cli_...` 令牌（名称 `"Pandar CLI"`，scope `["cli:session"]`）；
  5. 写入审计事件 `cli_login_ticket.exchange`；
  6. 任一步骤失败整体回滚，不留孤儿令牌，也不提前消费票据。
- 成功响应：`200 OK` → `{"token": "pandar_cli_...", "expires_at": "...", "profile": {"user_id": "...", "user_name": "...", "tenant_id": "...", "tenant_name": "..."}}`（`user_name` 为签发用户真实显示名）；
- 错误响应：`400 {"error":"bad_request"}`、`401 {"error":"invalid_plugin_ticket"}`。

### 5. `DELETE /api/v1/cli/session`

挂载在顶层 `router` 下（OpenAPI `security: [{"bearerAuth": []}]`）。

- 鉴权：直接通过 `bearer_token(&headers)?` 提取明文令牌，**绕过 no-auth 旁路**，调用专用的 `AuthRepository::revoke_cli_session_token_with_audit(plaintext_token)`（不复用仅支持 `plugin:studio` 的方法，也不经过滤 `revoked_at IS NULL` 的 `authenticate_tenant_token`）；
- 精确语义：
  - 按 `token_hash` 查询 `tenant_tokens`（**不**过滤 `revoked_at` 与 `expires_at`）；
  - 查无记录，或 `token.scopes != [TenantTokenScope::CliSession]`（包含所有 `pandar_tenant_...` API Key、`web:session`、`mobile:session`、`plugin:studio`）→ 返回 `401 {"error":"invalid_auth_token"}`；
  - 命中 `CliSession` 且 `revoked_at IS NULL` → 在 `begin_write_transaction()` 中条件更新 `revoked_at`（`WHERE revoked_at IS NULL`），当 `rows_affected == 1` 时记录一次 `tenant_token.revoke` 审计事件，返回 `204 No Content`；
  - 命中同一 `CliSession` 且 `revoked_at IS NOT NULL`（重复调用或并发注销竞争的后者）→ 幂等返回 `204 No Content`，不写重复审计事件；
  - 命中已过期但属于 `CliSession` 的令牌 → 同样执行上述逻辑并返回 `204 No Content`，确保过期会话可被干净登出。

### 6. OpenAPI 3.1 契约与客户端代码生成

在 `contracts/hub-client.openapi.json` 中同步更新，并在 `scripts/generate-hub-clients.mjs` 的 `validateContract()` 中强制校验每条 operation 均具备合法的 `security` 与 `x-pandar-no-auth` 声明（DTO 代码生成仍消费 `components.schemas`，不影响既有生成客户端类型）：

1. 新增 3 个互不混用的 `components.securitySchemes`，由 scheme 本身表达凭据边界：
   - `externalJwt`：`{"type": "http", "scheme": "bearer", "bearerFormat": "JWT"}`（外部身份提供商 JWT）；
   - `tenantToken`：`{"type": "http", "scheme": "bearer"}`（租户 API 密钥 `pandar_tenant_*` 或浏览器签发的会话令牌 `pandar_cli_*` / `pandar_web_*` / `pandar_mobile_*`）；
   - `bootstrapToken`：`{"type": "http", "scheme": "bearer"}`（Hub Bootstrap Token）；
2. **禁止在 `security` 中使用空对象 `{}`**（避免 OpenAPI 语义误判为“正常认证模式下也允许匿名”）；对所有非公开 operation 统一增加机器可校验扩展字段 `"x-pandar-no-auth": "allowed" | "forbidden"`，明确表达仅当 Hub 开启 `PANDAR_HUB_NO_AUTH` 本地模式时是否允许匿名；
3. 为 `contracts/hub-client.openapi.json` 中的**全部**现有与新增路径按 6 类精确声明：
   - **A. 始终公开**：`security: []`（不设 `x-pandar-no-auth`）
     - `GET /api/v1/auth/status`、`POST /api/v1/mobile/login-tickets/exchange`、`POST /api/v1/web/login-tickets/exchange`、`POST /api/v1/cli/login-tickets/exchange`；
   - **B. 仅 Tenant Token（no-auth 下也必须携带有效令牌）**：`security: [{"tenantToken": []}]` + `"x-pandar-no-auth": "forbidden"`
     - `GET /api/v1/auth/whoami`、`DELETE /api/v1/cli/session`；
   - **C. 仅外部用户 JWT（拒绝 Tenant Token，no-auth 下亦禁止匿名）**：`security: [{"externalJwt": []}]` + `"x-pandar-no-auth": "forbidden"`
     - `GET /api/v1/me`、`POST /api/v1/onboarding/tenants`、`POST /api/v1/join-links/accept`、`POST /api/v1/tenants/{tenant_id}/cli/login-tickets`（no-auth 下返回 `403 external_auth_required`）；
   - **D. 仅外部用户 JWT（拒绝 Tenant Token；no-auth 本地模式允许匿名）**：`security: [{"externalJwt": []}]` + `"x-pandar-no-auth": "allowed"`
     - `GET/POST /api/v1/tenants/{tenant_id}/join-links`、`POST /api/v1/tenants/{tenant_id}/{plugin,mobile,web}/login-tickets`；
   - **E. 外部用户 JWT 或 Tenant Token（no-auth 本地模式允许匿名）**：`security: [{"externalJwt": []}, {"tenantToken": []}]` + `"x-pandar-no-auth": "allowed"`
     - `contracts/hub-client.openapi.json` 中全部既有租户资源端点：`GET /printers`、`GET/PATCH/DELETE /printers/{printer_id}`、`POST /printers/{printer_id}/controls`、`GET/DELETE /jobs`、`POST /jobs/{job_id}/retry-dispatch`、`POST /jobs/{job_id}/reprint`、`GET/POST /agents`、`POST /agents/{agent_id}/{discover-printers,diagnose-printer,link-printer}`、`POST /agent-pairings`、`GET /commands/{command_id}`、`GET/POST /tenant-tokens`、`POST /tenant-tokens/{token_id}/rotate`、`GET /users`、`GET /audit-events`、`POST /artifact-metadata-preview`；
   - **F. Bootstrap Token（认证模式拒绝 Tenant Token；no-auth 本地模式允许匿名）**：`security: [{"bootstrapToken": []}]` + `"x-pandar-no-auth": "allowed"`
     - `GET /api/v1/tenants`；
4. **明确当前不在 `contracts/hub-client.openapi.json` 契约范围内的运行时端点**（本期不强行扩编 OpenAPI，仍由 Hub 路由测试覆盖权限矩阵）：
   - 运行时属 D 类：`PATCH /api/v1/tenants/{tenant_id}`、`DELETE /api/v1/tenants/{tenant_id}/join-links/{join_link_id}`；
   - 运行时属 E 类：`GET /api/v1/tenants/{tenant_id}/printer-events`、`POST /api/v1/tenants/{tenant_id}/printer-events/tickets`、`GET /api/v1/tenants/{tenant_id}/printers/{printer_id}/camera.mp4`、`POST /api/v1/tenants/{tenant_id}/printers/{printer_id}/materials:refresh`、`POST /api/v1/tenants/{tenant_id}/printers/{printer_id}/jobs`、`GET/DELETE /api/v1/tenants/{tenant_id}/jobs/{job_id}`、`POST /api/v1/tenants/{tenant_id}/jobs/{job_id}/duplicate`、`DELETE /api/v1/tenants/{tenant_id}/tenant-tokens/{token_id}`、`DELETE /api/v1/tenants/{tenant_id}/agents/{agent_id}`、`POST /api/v1/tenants/{tenant_id}/agents/{agent_id}/refresh-printers`、`POST /api/v1/tenants/{tenant_id}/agents/{agent_id}/credential:{rotate,revoke}`、`DELETE/PATCH/GET /api/v1/tenants/{tenant_id}/users/{user_id}*`；
   - 独立鉴权边界：`/api/v1/agents/{agent_id}/*`（Agent credential）与 `/api/v1/plugin/*`（Studio 插件）。
5. 为新增路径声明完整错误响应码：
   - `GET /api/v1/auth/whoami`：响应 `200`、`401`、`default`；
   - `POST /api/v1/tenants/{tenant_id}/cli/login-tickets`：响应 `201`、`400`、`401`、`403`、`default`；
   - `POST /api/v1/cli/login-tickets/exchange`：响应 `200`、`400`、`401`、`default`；
   - `DELETE /api/v1/cli/session`：响应 `204`、`401`、`default`；
6. 新增/收紧 schema（严格遵循 OpenAPI 3.1 null union，不使用 `nullable`）：
   - 新增 `TenantTokenScope` 枚举（`"*"`、`"operator"`、`"viewer"`、`"agent:register"`、`"plugin:studio"`、`"mobile:session"`、`"web:session"`、`"cli:session"`），在 `TenantToken.scopes` 与 `WhoamiResponse.scopes` 中引用；
   - `WhoamiResponse`：`expires_at` 为 `["string", "null"]`，`effective_role` 为 `["string", "null"]`，`user` 为 `oneOf: [{"$ref": "#/components/schemas/WhoamiUser"}, {"type": "null"}]`；
   - `CliLoginTicketRequest`、`CliTicketExchangeRequest`、`CliTicketExchangeResponse` 归属 `x-typescript-group: "core"`；
   - **明确不加入 `x-pandar-codegen.kotlinRoots`**：上述 CLI 专用 schema 不影响 Android 客户端生成代码；
7. 运行 `npm run generate:hub-clients` 生成 TypeScript 类型，并通过 `npm run check:hub-clients`。

## Web 前端设计

### 1. CLI 浏览器登录页（`/cli-sign-in`）

镜像现有 `/web-sign-in`（`frontend/app/cli-sign-in/page.tsx` 与 `cli-ticket-form.tsx`）：

- 入口参数：`/cli-sign-in?redirect_url=&code_challenge=&state=&tenant=`；
- 缺少 `redirect_url` 时不请求 Hub，直接渲染缺失回调错误态；
- 本地 no-auth 模式（`auth.source === "none" && provider.provider === "none"`）时，渲染明确提示引导使用 API Key 登录，不提交必定失败的 CLI 票据请求；
- 未登录外部认证时展示登录引导；
- 单租户自动选中，多租户复用 `TenantTicketSelection`；
- 提交表单调用 `createCliTicket` Server Action（扩展 `createSessionTicket(formData, "cli")`），成功后重定向至 `{redirect_url}?ticket=...&redirect_url=...&state=...`，Hub 拒绝时重定向到 `/devices?status=<error>`。

### 2. API 密钥 / 租户令牌管理界面优化

- **令牌列表**（`frontend/app/admin-settings-token-list.tsx`）：
  - 补充 `operator`、`viewer`、`cli:session`、`web:session`、`mobile:session` 的中英文标签，不再回落显示原始 scope 字符串。
- **创建对话框**（`frontend/app/admin-token-dialogs.tsx` 与 `admin-panel-shared.tsx`）：
  - 扩展共享 `Input` 组件支持可选受控属性 `value` / `onChange`；
  - 将 `CreateTenantTokenDialogSession` 中的 `Scopes` 改为受控状态（默认 `"*"`），新增快捷权限预设按钮：`完整权限 (*)`、`操作员 (operator)`、`查看者 (viewer)`、`Agent 注册 (agent:register)`；
  - 点击预设按钮将该预设填入输入框（与直接手输组合兼容）；提交时在 `createTenantToken` 中统一执行 trim、按规范顺序去重（与 Hub 端规范顺序一致），确保提交干净的 `scopes` 数组；
  - 同步维护 `frontend/messages/en.json` 和 `frontend/messages/zh.json`。

## Pandar CLI 设计（`crates/pandar-app`）

### 1. 安全凭据封装与输入卫生

- 令牌/API Key 在内存中封装为 `SecretString`（自定义 `Debug` 输出 `Secret([redacted])`、`ZeroizeOnDrop`），确保不会通过 `Debug`、`tracing`、错误 context 或 panic 泄露明文；
- `--api-key` 命令行参数与 `--api-key-stdin` 互斥（`conflicts_with`）；在 CLI help 与文档中明确标注 `--api-key` 会进入 shell 历史与同机进程列表，自动化/共享主机场景推荐使用 `--api-key-stdin` 或环境变量 `PANDAR_API_KEY`；
- **`--api-key-stdin` 严格读取规则**（仅 `pandar login` 提供，不出现在 `pandar client` 上，从而避免与 `pandar client request --data-file -` 争抢 stdin）：
  - 单次读取 stdin，设置 `4096` 字节上限（超限即报错，防止误传大文件）；
  - 仅剥离末尾单个 `\r\n` 或 `\n`；
  - 剥离后若为空、仍包含任何换行/回车/空白字符（例如误传入多把密钥或整份配置文件），一律拒绝报错；
  - 读取 I/O 错误返回明确上下文，绝不在错误消息中回显任何已读字节。

### 2. 本地配置、权限保护与并发控制

- 配置文件路径优先级：`--config <PATH>` > `PANDAR_CLI_CONFIG` > 平台默认路径（`$XDG_CONFIG_HOME/pandar/cli.json` / `$HOME/.config/pandar/cli.json` / `%APPDATA%\pandar\cli.json`）；
- `CliConfig` / `CliProfile` 使用 `#[serde(deny_unknown_fields)]`：
  - `web_url` 定义为 `Option<String>`（API Key 直连 Hub 时为 `null`/省略）；
  - 配置文件 JSON 损坏或含未知 `auth_kind` 时 fail-closed 返回含路径的明确解析错误，绝不静默覆盖；
  - 缺失 `active_profile` 或指向不存在的 profile 时返回清晰提示（引导指定 `--profile` 或执行 `pandar login`）。
- **文件系统安全校验（覆盖目录、配置文件与锁文件全部路径组件）**：
  - 读写与加锁前均校验目标 `pandar` 父目录、配置文件本身及 `<config_filename>.lock` 均**不是符号链接**（Unix 上额外通过 `O_NOFOLLOW` 打开既有配置文件与锁文件以消除 TOCTOU 竞态；Windows 上拒绝 reparse point / symlink）；
  - **Unix**：
    - 读取既有配置文件及打开锁文件前校验文件属主为当前有效 UID（`uid == geteuid()`）且权限无 group/other 位（`mode & 0o077 == 0`）；若权限过宽或属主不匹配，直接 fail-closed 并提示执行 `chmod 600 <path>`；
    - 写入时创建 `0o700` 父目录、以 `OpenOptionsExt::mode(0o600)` + `create_new(true)` 创建同目录临时文件、写入并 `sync_all()`、二次校验权限后原子 `rename` 覆盖目标；
  - **Windows**：
    - 写入当前用户专属目录 `%APPDATA%\pandar\cli.json`（继承用户目录私有 ACL），同样拒绝符号链接并执行临时文件原子替换；在文档中明确标注 Windows 依赖用户目录默认 DACL 而非 POSIX mode bits。
- **跨进程并发写锁**：
  - 在 `login` / `logout` 的读-改-写临界区外层获取同目录 `<config_filename>.lock` 排他文件锁（Unix 使用 `O_NOFOLLOW | mode(0o600)` 打开并校验属主/权限后加 `flock` / `File::lock`，Windows 使用 `LockFileEx` / `File::lock`），确保并发更新不同 profile 不会发生丢写。

### 3. URL 校验、发现信任边界与重定向策略

- **基础 URL 校验（`normalize_base_url`）**：仅允许 `http` / `https`、非空 host、非零端口（若显式指定），拒绝 userinfo（`user:pass@`）、query（`?`）与 fragment（`#`）。
- **HTTP 客户端统一禁止自动跟随重定向**：所有 CLI `reqwest::Client` 均配置 `reqwest::redirect::Policy::none()`，防止 3xx 跳转将 Bearer 凭据或一次性票据泄漏给第三方主机。
- **URL 发现与信任边界**：
  - **API Key 模式**：
    - `--server` 默认直接视为 Hub API 基础 URL，**绝不**把 API Key 发往 `/.well-known/pandar` 返回的任意跨主机地址；
    - 若显式传入 `--hub-url`，直接使用 `--hub-url`（并将 `--server` 记为 `web_url`）；
    - 未传 `--hub-url` 时，先探测 `GET {server}/.well-known/pandar`：
      - 若返回 `200 OK`：响应体**必须**是合法 JSON 且包含非空合法 `http(s)` 字符串字段 `hubUrl`，同时必须与 `--server` **完全同源**（同 scheme、同 host、同端口）；若 `200 OK` 响应为非法 JSON、缺少/空/非字符串 `hubUrl`、非法 URL 或跨源 `hubUrl`，**直接报错退出，禁止回退到 `--server`**；
      - 仅当 `/.well-known/pandar` 返回 `404 Not Found`（或表示该服务无发现端点的其他非 2xx 状态）时，将 `--server` 直接作为 `hub_url`（`web_url = None`）。
  - **浏览器交互模式**：
    - `--server` 视为 Pandar Web URL（保存到 `web_url`）；
    - 若传入 `--hub-url`，直接使用；
    - 否则请求 `{server}/.well-known/pandar` 获取 `hubUrl` 并严格校验：
      - 非 `200 OK`、非法 JSON、缺少/空/非字符串 `hubUrl` 或非法 URL 一律直接报错退出；
      - 若 `--server` 为 `https`，`hubUrl` 必须为 `https`（禁止降级为明文 `http`）；
      - 仅允许与 `--server` 同主机，或二者均为本机回环地址（`localhost`/`127.0.0.1`/`[::1]`，适配本地开发 `3000` → `8080`）；其他跨主机 `hubUrl` 拒绝自动信任并提示显式传入 `--hub-url`。
- **优先级规则**（`pandar client`）：
  - `hub_url`：`--hub-url` / `PANDAR_HUB_URL` > `--server` / `PANDAR_SERVER` > 选中 Profile 的 `hub_url`；
  - `token`：`--api-key` / `PANDAR_API_KEY` > 选中 Profile 的 `token`；
  - `tenant_id`：`--tenant-id` / `PANDAR_TENANT_ID` > 选中 Profile 的 `tenant_id`（若采用环境变量无状态调用且未传 `tenant_id`，自动通过 `GET {hub_url}/api/v1/auth/whoami` 解析）。
  - 后续若从已有 profile 发起浏览器登录但 `web_url` 为 `None`，要求用户显式提供 `--server <web_url>`。

### 4. `pandar login` 与本机一次性回调服务

```bash
# 方式一（自动化推荐）：通过 stdin 或环境变量传入 API Key
printf '%s' "$PANDAR_API_KEY" | pandar login --server https://api.pandar.example --api-key-stdin
PANDAR_API_KEY=pandar_tenant_xxx pandar login --server https://api.pandar.example

# 方式二（本地便捷调试，help 中标注进程列表/历史风险）
pandar login --server https://api.pandar.example --api-key pandar_tenant_xxx

# 方式三：浏览器 PKCE 交互登录
pandar login --server https://pandar.example [--hub-url https://api.pandar.example] [--tenant-id <uuid>] [--no-browser] [--profile work]
```

本机一次性回调服务器（`127.0.0.1:0`）行为约束：

- 仅受理 `GET /callback`，其他方法返回 `405 Method Not Allowed`，其他路径返回 `404 Not Found`；
- 原始 query 长度超过 `4096` 字节返回 `414 URI Too Long`；
- 自行解析原始 query 键值，拒绝重复的 `ticket` 或 `state` 参数、缺失 `ticket`/`state` 或 `state` 不匹配：返回 `400 Bad Request` HTML 页面（不泄露期望的 `state`），且**不中止等待**，防止本地无关或恶意探测请求打断正在进行的登录；
- 首次收到 `state` 匹配且非空 `ticket` 的请求：完成 oneshot 通知，返回 `200 OK` HTML 成功页并触发优雅关闭；此后在关闭窗口内到达的重复回调返回 `410 Gone` HTML，不再重复处理；
- 等待超时（默认 5 分钟）时立即触发 shutdown 并回收监听端口；
- 任何日志与报错（包含浏览器启动失败提示）均**禁止**输出 `ticket`、`code_verifier` 或含敏感参数的完整回调 query。

### 5. `pandar logout`

```bash
pandar logout [--profile <name>] [--local-only] [--config <path>]
```

- 在排他文件锁内读取配置、移除目标 profile 并更新 `active_profile`；
- 若 `auth_kind == "browser_session"` 且未传 `--local-only`，调用 `DELETE {hub_url}/api/v1/cli/session`；远端失败时向 stderr 输出脱敏警告但仍完成本地清理；
- 若 `auth_kind == "api_key"`，仅删除本地配置，绝不吊销远端 API Key。

### 6. `pandar client`

全局选项：`--profile`、`--config`、`--server`、`--hub-url`、`--api-key`、`--tenant-id`、`--timeout-seconds`（默认 `60`，multipart 上传默认 `300`）。

#### 6.1 `whoami` 与通用 `request`

```bash
pandar client whoami

pandar client request <METHOD> <PATH> \
  [-d|--data <JSON>] \
  [--data-file <PATH|->] \
  [-H|--header <KEY:VALUE>]... \
  [-q|--query <KEY=VALUE>]... \
  [--no-auth] \
  [--include]
```

严格安全与路径规则：

- **路径校验**：
  - 先拒绝空字符串、任何绝对 URL（含 scheme `:` 如 `http://`、`https://`、`file:`）、原始反斜杠 `\`、原始空白或控制字符、fragment `#`、query `?`（查询参数统一通过 `-q KEY=VALUE` 传递）以及非法百分号转义（非完整 `%XX` 十六进制）；
  - 剥离最多一个开头 `/` 与最多一个尾随 `/`（若剥离后为空，例如单独 `/` 或空串，则直接拒绝）；
  - 对剩余字符串按 `/` 切分路径段：
    - 若出现任意空段（自动拒绝协议相对路径 `//evil.com/x` 以及连续斜杠 `/printers//123`），直接拒绝；
    - 对每个路径段执行一次严格百分号解码，拒绝解码后等于 `.` 或 `..`（覆盖 `..`、`%2e%2e`、`%2e.`、`.%2e` 等全部混合编码），以及解码后包含 `/`、`\` 或 ASCII 控制字符（`< 0x20` 或 `0x7f`）的路径段；
  - 合法示例：`printers`、`/printers`、`/printers/`、`/printers/00000000-0000-4000-8000-000000000001`、`/api/v1/auth/whoami`、`/api/v1/auth/status`；
  - 拒绝示例：`/`、`//evil.com/printers`、`/printers//123`、`../tenants`、`/printers/%2e./jobs`、`/printers/a%2fb`、`https://evil.com/x`、`/printers?foo=1`；
  - 以 `/api/` 开头（规范化后以 `api/` 开头）：发往 `{hub_url}/{normalized_path}`；
  - 其他相对路径：发往 `{hub_url}/api/v1/tenants/{tenant_id}/{normalized_path}`。
- **Header 校验与输出脱敏**：
  - 默认自动附带 `Authorization: Bearer <token>`；
  - 禁止通过 `-H` 设置或覆盖 `Authorization`、`Host`、`Cookie`（若需访问 `/api/v1/auth/status` 等公开无认证端点，使用 `--no-auth` 不发送 Bearer 头）；
  - 携带 `-d` / `--data-file` 时默认附带 `Content-Type: application/json`；
  - `--include` 将状态码与响应头输出到 stderr，并对 `set-cookie`、`authorization`、`www-authenticate`、`proxy-authenticate` 的值脱敏为 `[redacted]`；
- **超时与重定向**：连接超时 `10s`，整体请求超时遵循 `--timeout-seconds`（默认 `60s`），`redirect::Policy::none()`；HTTP 非 2xx 时仍输出响应体并以非零退出码返回。

#### 6.2 常用资源结构化子命令

```bash
# 打印机
pandar client printers list
pandar client printers get <printer_id>
pandar client printers control <printer_id> --action <action> [--payload <json>]
pandar client printers refresh-materials <printer_id>
pandar client printers delete <printer_id>

# 打印任务
pandar client jobs list
pandar client jobs get <job_id>
pandar client jobs create <printer_id> \
  --file <path> \
  [--filename <name>] \
  [--content-type <mime>] \
  [--plate <i64>] \
  [--use-ams <bool>] \
  [--bed-leveling <bool>] \
  [--auto-bed-leveling <0|1|2>] \
  [--flow-cali <bool>] \
  [--auto-flow-cali <0|1|2>] \
  [--auto-offset-cali <0|1|2>] \
  [--timelapse <bool>] \
  [--ams-mapping <json>] \
  [--ams-mapping2 <json>] \
  [--ams-mapping-info <json>]
pandar client jobs retry <job_id> [--reason <text>]
pandar client jobs reprint <job_id> [--reason <text>]
pandar client jobs delete <job_id>

# Agent
pandar client agents list
pandar client agents refresh-printers <agent_id>
pandar client agents discover-printers <agent_id> [--timeout-seconds <n>]
pandar client agents diagnose-printer <agent_id> --serial <serial>

# API 密钥（租户令牌）管理
pandar client tokens list
pandar client tokens create --name <name> [--scopes <s1,s2>] [--expires-at <rfc3339>]
pandar client tokens rotate <token_id> [--expires-at <rfc3339>]
pandar client tokens revoke <token_id>

# 异步命令查询
pandar client commands get <command_id>
```

`jobs create` 上传约束（严格对齐 `crates/pandar-hub/src/routes/jobs/multipart.rs`）：

- `--file <path>` 必须指向存在的非空常规文件（支持从本地文件流式上传，不支持 stdin 以确保内容长度与文件名可靠）；
- 默认取文件 basename 作为 `filename`（可用 `--filename` 覆盖），根据扩展名推断 `content_type`（`.3mf` → `model/3mf`，`.gcode` → `text/x-gcode`，其余 → `application/octet-stream`，可用 `--content-type` 覆盖）；
- 完整附带 Hub 必需的全部 multipart 字段（`file`、`plate_id`、`use_ams`、`bed_leveling`、`auto_bed_leveling`、`flow_cali`、`auto_flow_cali`、`auto_offset_cali`、`timelapse` 及可选 AMS 映射 JSON）；
- 目标为 Moonraker 打印机时，Hub 仅接受 `.gcode` 文件且拒绝 AMS 映射，CLI 原样透传 Hub 错误码（如 `artifact_invalid_upload` / `artifact_too_large` / `bad_request`）并以非零状态退出。

## 模块拆分计划（满足 400 LOC 约束）

遵循仓库 400 LOC/文件限制（通过仓库既有的 `cargo run -p pandar-quality -- module-size` 强制检验）且不新增外部依赖：

1. **Hub 路由层**：
   - `crates/pandar-hub/src/routes/auth.rs`（当前 400 行）→ 拆出 `crates/pandar-hub/src/routes/auth/external.rs`（`bearer_token`、`verify_external_identity`、`external_profile`、`authenticate_bearer_tenant_token`），`auth.rs` 降至 ~320 行；
   - `crates/pandar-hub/src/routes/plugin.rs`（当前 367 行）→ 拆出 `crates/pandar-hub/src/routes/plugin/ticket_helpers.rs`（共享 PKCE/过期时间/错误映射/user_id 辅助函数），新增 `crates/pandar-hub/src/routes/plugin/cli.rs`（~110 行），`plugin.rs` 降至 ~320 行；
   - 新增 `crates/pandar-hub/src/routes/whoami.rs`（~80 行，独立响应模型与专用鉴权路径）。
2. **Hub 仓储层**：
   - `crates/pandar-hub/src/repositories/auth/plugin_tickets.rs`（当前 387 行）→ 拆出 `crates/pandar-hub/src/repositories/auth/plugin_tickets/helpers.rs`（模型转换、PKCE 校验、审计构造、写入），新增 `cli.rs`（~100 行），并在 `exchange_login_ticket` 中切换为 `begin_write_transaction()`，`plugin_tickets.rs` 降至 ~280 行；
   - 新增 `crates/pandar-hub/src/repositories/auth/tenant_tokens/cli.rs`（~40 行）；
   - 更新 `crates/pandar-hub/src/repositories/auth/tenant_tokens/revocation.rs`（引入 `begin_write_transaction()` 与 `revoke_cli_session_token_with_audit`）。
3. **CLI（`crates/pandar-app`）**：
   - 仅复用根 `Cargo.toml` 既有 workspace 依赖（`axum`、`base64`、`reqwest`、`sha2`、`uuid`、`zeroize`、`tempfile`）；
   - 拆分为 `src/cli_auth.rs`、`src/cli_auth/config.rs`、`src/cli_auth/lock.rs`、`src/cli_auth/browser.rs`、`src/client.rs`、`src/client/request.rs`、`src/client/commands.rs`，每文件均控制在 300 行以内。
4. **前端（`frontend/`）**：
   - 新增 `frontend/app/cli-sign-in/{page.tsx,cli-ticket-form.tsx,page.test.tsx}`；
   - 最小化修改 `actions.ts`、`admin-panel-shared.tsx`、`admin-settings-token-list.tsx`、`admin-token-dialogs.tsx`、`messages/{en,zh}.json` 及生成类型文件。

## 验收标准

- [ ] **No-auth 安全隔离**：Hub 开启 `no_auth` 时，`GET /api/v1/auth/whoami` 和 `DELETE /api/v1/cli/session` 对缺失、格式错误、无效令牌均返回 `401`，有效令牌返回 `200`/`204`；`POST /tenants/{id}/cli/login-tickets` 在 `no_auth` 下返回 `403 external_auth_required`。
- [ ] **API 密钥完整权限矩阵与规范化**：覆盖 `*`、`operator`、`viewer`、`[]`、`agent:register`、`plugin:studio`、混合/重复 scope，在打印机、任务、Agent、命令、令牌、用户、审计、租户更新、加入链接与登录票据全类别路由上验证放行与 `403`；手工创建 `cli:session`/`web:session`/`mobile:session` 返回 `400 invalid_scope`。
- [ ] **CLI 登录票据与会话生命周期**：
  - 外部 `Viewer`、`Operator`、`TenantAdmin` 可创建 CLI 票据，任意 `TenantToken` 返回 `403`，跨租户返回 `403`；
  - 非回环地址、零端口、带 query/fragment/userinfo 的 `redirect_url` 返回 `400 invalid_redirect_url`；
  - 错误 `code_verifier`、过期、重复使用、跨类型票据交换返回 `401 invalid_plugin_ticket`；
  - 绑定用户角色降级后权限实时降级；用户被删除后业务路由与 `whoami` 均返回 `401 invalid_auth_token`；
  - `DELETE /api/v1/cli/session` 首次与重复/并发注销均返回 `204` 且仅写一条审计记录；传入 `pandar_tenant_...` API Key 返回 `401`。
- [ ] **双数据库（SQLite + PostgreSQL）并发与回滚等价**：
  - 并发交换同一 CLI 票据时有且仅有一个成功，其余返回 `401`；
  - 并发注销同一 `cli:session` 均返回 `204` 且不触发 `SQLITE_BUSY_SNAPSHOT`；
  - 事务中途失败时票据与令牌状态完整回滚；
  - 既有历史 `tenant_tokens` 与 `plugin_login_tickets` 数据可安全读取与鉴权。
- [ ] **CLI 安全边界与端到端链路**（真实启动 `pandar-hub` HTTP 服务并运行 `pandar` 进程）：
  - 恶意 `/.well-known/pandar` 指向跨主机地址时，API Key 不会被发送且登录失败；3xx 重定向不会被自动跟随；
  - `--api-key-stdin` 与 `PANDAR_API_KEY` 登录成功写入 `0o600` 文件；拒绝符号链接与过宽权限（如 `0o644`）配置文件；并发写入不同 profile 不丢更新；stdout/stderr 与错误输出不含明文密钥；
  - 浏览器登录本地回调正确处理非 `/callback`（404）、非 GET（405）、错误/缺失/重复 `state`（400 且继续等待）、有效回调（200）、重复回调（410）与超时关闭端口，且完成真实 PKCE exchange；
  - `pandar client whoami`、`request`（含路径穿越/绝对 URL/受保护头拒绝与 `--include` 响应头脱敏）、`printers`、`jobs create`（真实 multipart `.3mf`/`.gcode` 上传到 Hub 成功创建任务）、`agents`、`tokens`、`commands` 及无配置 CI 环境变量模式全量跑通；
  - `pandar logout` 对浏览器会话远端吊销、对 API Key 仅本地移除；
  - 产出可重复验证的 E2E 测试产物：`target/e2e-artifacts/cli-api-key-flow.json`。
- [ ] **OpenAPI 与前端**：
  - `npm run check:hub-clients` 通过，OpenAPI 含 `securitySchemes`、路径安全声明、错误码与 `TenantTokenScope` 枚举；
  - `/cli-sign-in` 页面与 `createCliTicket` 测试通过，令牌列表新 scope 徽章与创建弹窗预设按钮提交真实 scopes 数组测试通过；`lint:web` 与 `typecheck:web` 零警告。
- [ ] **质量门禁**：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo run -p pandar-quality -- module-size` 与全量 `cargo nextest` 全部通过。

## 验证命令

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p pandar-quality -- module-size
cargo nextest run --manifest-path Cargo.toml --workspace

# SQLite + PostgreSQL 双后端专项门禁（禁止因缺失变量或过滤器空匹配而静默跳过）
cargo nextest run -p pandar-hub --no-tests=fail -E 'test(cli_sign_in) | test(tenant_token) | test(whoami)'
PANDAR_REQUIRE_POSTGRES_TESTS=true \
PANDAR_TEST_POSTGRES_URL=postgres://... \
cargo nextest run -p pandar-hub --no-tests=fail -E 'test(postgres_cli_login_tickets) | test(postgres_tenant_tokens)'

# CLI 端到端验收（产出 target/e2e-artifacts/cli-api-key-flow.json）
cargo nextest run -p pandar-app --no-tests=fail

# OpenAPI 与 Web 前端门禁
npm run check:hub-clients
npm run lint:web
npm run typecheck:web
npm run test:web
```

## 文档影响

实现并验收完成后同步更新：

- `docs/roadmap.md`：记录 API 密钥增强与 `pandar login` / `pandar client` 交付结果及 E2E 产物路径；
- `docs/development.md`：补充 `pandar login` / `pandar client` 使用与安全提示（优先使用 `--api-key-stdin` 或环境变量）。

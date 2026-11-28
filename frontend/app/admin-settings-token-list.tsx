import { ShieldCheckIcon, TriangleAlertIcon, XCircleIcon } from "lucide-react";
import { useLocale, useTranslations } from "next-intl";

import { FormattedDate } from "../components/formatted-date";
import { useAdminDate } from "./admin-model";
import {
  CreateTenantTokenDialog,
  RevokeTenantTokenDialog,
  RotateTenantTokenForm,
} from "./admin-token-dialogs";
import type { TenantToken } from "./dashboard-types";
import { getRelativeTime } from "./dayjs-relative-time";

function TokenExpiration({ value, nowMs }: { value: string; nowMs: number }) {
  const t = useTranslations("admin");
  const locale = useLocale();
  const formatDate = useAdminDate();
  const relativeTime = getRelativeTime(value, nowMs, locale);

  if (!relativeTime) {
    return <>{t("expires", { date: formatDate(value) })}</>;
  }

  const label =
    relativeTime.timestampMs <= nowMs ? "expiredRelative" : "expiresRelative";
  return (
    <time dateTime={value} title={formatDate(value)}>
      {t(label, { relative: relativeTime.relative })}
    </time>
  );
}

type TokenStatus = "active" | "expired" | "revoked";

const TOKEN_STATUS_STYLES = {
  active: "border-success/40 bg-success/10 text-success",
  expired: "border-warning/50 bg-warning/10 text-warning",
  revoked: "border-border bg-muted text-muted-foreground",
} satisfies Record<TokenStatus, string>;

const TOKEN_STATUS_ICONS = {
  active: ShieldCheckIcon,
  expired: TriangleAlertIcon,
  revoked: XCircleIcon,
} satisfies Record<TokenStatus, typeof ShieldCheckIcon>;

const TOKEN_STATUS_ORDER: Record<TokenStatus, number> = {
  active: 0,
  expired: 1,
  revoked: 2,
};

function getTokenStatus(token: TenantToken, nowMs: number): TokenStatus {
  if (token.revoked_at) {
    return "revoked";
  }
  if (nowMs > 0 && token.expires_at && Date.parse(token.expires_at) <= nowMs) {
    return "expired";
  }
  return "active";
}

function TokenStatusBadge({ status }: { status: TokenStatus }) {
  const t = useTranslations("admin");
  const Icon = TOKEN_STATUS_ICONS[status];

  return (
    <span
      className={
        "inline-flex items-center gap-1 rounded-md border px-2 py-0.5 text-xs font-medium " +
        TOKEN_STATUS_STYLES[status]
      }
    >
      <Icon aria-hidden="true" className="size-3" />
      {t("tokenStatus." + status)}
    </span>
  );
}

function TokenScope({ scope }: { scope: string }) {
  const t = useTranslations("admin");
  const labels: Record<string, string> = {
    "*": t("scopeAll"),
    operator: t("scopeOperator"),
    viewer: t("scopeViewer"),
    "agent:register": t("scopeAgentRegister"),
    "plugin:studio": t("scopePluginStudio"),
    "cli:session": t("scopeCliSession"),
    "web:session": t("scopeWebSession"),
    "mobile:session": t("scopeMobileSession"),
  };

  return (
    <span className="inline-flex rounded-md border border-border bg-muted/60 px-2 py-0.5 text-xs font-medium text-muted-foreground">
      {labels[scope] ?? scope}
    </span>
  );
}

function TokenLastUsed({
  value,
  nowMs,
}: {
  value: string | null;
  nowMs: number;
}) {
  const t = useTranslations("admin");
  const locale = useLocale();
  const formatDate = useAdminDate();
  if (!value) {
    return <>{t("lastUsedNever")}</>;
  }
  const relativeTime = getRelativeTime(value, nowMs, locale);
  return (
    <time dateTime={value} title={formatDate(value)}>
      {relativeTime?.relative ?? formatDate(value)}
    </time>
  );
}

export function TenantTokensTable({
  tenantId,
  tokens,
  nowMs,
}: {
  tenantId: string;
  tokens: TenantToken[];
  nowMs: number;
}) {
  const t = useTranslations("admin");
  const sortedTokens = [...tokens].sort((left, right) => {
    const statusDifference =
      TOKEN_STATUS_ORDER[getTokenStatus(left, nowMs)] -
      TOKEN_STATUS_ORDER[getTokenStatus(right, nowMs)];
    return (
      statusDifference ||
      Date.parse(right.created_at) - Date.parse(left.created_at) ||
      right.id.localeCompare(left.id)
    );
  });
  const activeCount = sortedTokens.filter(
    (token) => getTokenStatus(token, nowMs) === "active",
  ).length;

  return (
    <div>
      <div className="flex flex-col gap-3 pb-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-foreground">
            {t("tenantTokens")}
          </h3>
          <p className="mt-0.5 text-xs text-muted-foreground">
            {t("tenantTokensDescription")} ·{" "}
            {t("tenantTokensSummary", {
              active: activeCount,
              total: tokens.length,
            })}
          </p>
        </div>
        <div className="shrink-0">
          <CreateTenantTokenDialog tenantId={tenantId} />
        </div>
      </div>
      {tokens.length === 0 ? (
        <div className="border-t border-border py-10 text-center">
          <div className="text-sm font-semibold text-foreground">
            {t("noTokensTitle")}
          </div>
          <p className="mx-auto mt-2 max-w-md text-sm text-muted-foreground">
            {t("noTokensMessage")}
          </p>
        </div>
      ) : (
        <div className="divide-y divide-border border-t border-border">
          {sortedTokens.map((token) => {
            const status = getTokenStatus(token, nowMs);
            return (
              <article
                key={token.id}
                className="flex flex-col gap-3 py-4 text-sm sm:flex-row sm:items-center sm:justify-between"
                data-token-id={token.id}
                data-token-status={status}
              >
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-semibold text-foreground">
                      {token.name}
                    </span>
                    <TokenStatusBadge status={status} />
                    {token.scopes.length === 0 ? (
                      <span className="inline-flex rounded-md border border-border bg-muted/60 px-2 py-0.5 text-xs font-medium text-muted-foreground">
                        {t("scopeReadOnly")}
                      </span>
                    ) : (
                      token.scopes.map((scope) => (
                        <TokenScope key={scope} scope={scope} />
                      ))
                    )}
                  </div>
                  <div className="mt-1.5 flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-xs text-muted-foreground">
                    <span>
                      {token.expires_at ? (
                        <TokenExpiration
                          value={token.expires_at}
                          nowMs={nowMs}
                        />
                      ) : (
                        t("expiresNever")
                      )}
                    </span>
                    <span aria-hidden="true">·</span>
                    <span>
                      {t("tokenLastUsedLabel")}{" "}
                      <TokenLastUsed value={token.last_used_at} nowMs={nowMs} />
                    </span>
                    <span aria-hidden="true">·</span>
                    <span>
                      {t("tokenCreatedLabel")}{" "}
                      <FormattedDate value={token.created_at} />
                    </span>
                  </div>
                  <div className="mt-1 text-xs text-muted-foreground">
                    {t("tokenIdLabel")}{" "}
                    <code className="break-all font-mono" title={token.id}>
                      {token.id}
                    </code>
                  </div>
                </div>
                <div className="flex shrink-0 flex-wrap items-center gap-2">
                  <RotateTenantTokenForm
                    tenantId={tenantId}
                    token={token}
                    status={status}
                  />
                  {status !== "revoked" ? (
                    <RevokeTenantTokenDialog
                      tenantId={tenantId}
                      token={token}
                    />
                  ) : null}
                </div>
              </article>
            );
          })}
        </div>
      )}
    </div>
  );
}

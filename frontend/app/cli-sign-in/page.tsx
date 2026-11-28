import type { ReactNode } from "react";
import { getTranslations } from "next-intl/server";

import { createCliTicket } from "../actions";
import { authSource } from "../api-auth";
import { authProviderConfig } from "../auth-provider";
import { firstParam, getSignInTenantContext } from "../dashboard-data";
import {
  TenantTicketSelection,
  TicketSignInEmptyState,
  TicketSignInFrame,
} from "../ticket-sign-in-ui";
import { CliTicketForm } from "./cli-ticket-form";

type PageProps = {
  searchParams?: Promise<{
    tenant?: string | string[];
    redirect_url?: string | string[];
    code_challenge?: string | string[];
    state?: string | string[];
  }>;
};

export default async function CliSignInPage({ searchParams }: PageProps) {
  const [t, params] = await Promise.all([
    getTranslations("signIn"),
    searchParams,
  ]);
  const redirectUrl = firstParam(params?.redirect_url);
  const frame = (content: ReactNode) => (
    <TicketSignInFrame
      title={t("cliTitle")}
      subtitle={t("cliSubtitle")}
      meta={t("cliMeta")}
    >
      {content}
    </TicketSignInFrame>
  );
  if (!redirectUrl) {
    return frame(
      <TicketSignInEmptyState
        title={t("cliMissingRedirectTitle")}
        message={t("cliMissingRedirectMessage")}
        statusLabel={t("actionRequired")}
        actions={[{ href: "/", label: t("returnDashboard") }]}
      />,
    );
  }

  const auth = await authSource();
  const provider = authProviderConfig();
  if (auth.source === "none" && provider.provider === "none") {
    return frame(
      <TicketSignInEmptyState
        title={t("cliNoAuthTitle")}
        message={t("cliNoAuthMessage")}
        statusLabel={t("actionRequired")}
        actions={[
          { href: "/settings", label: t("openAdmin") },
          { href: "/", label: t("returnDashboard") },
        ]}
      />,
    );
  }
  if (auth.source === "none") {
    return frame(
      <TicketSignInEmptyState
        title={t("authUnavailableTitle")}
        message={t("cliAuthMessage")}
        statusLabel={t("actionRequired")}
        actions={[
          ...(provider.signInUrl
            ? [{ href: provider.signInUrl, label: t("signIn") }]
            : []),
          { href: "/", label: t("returnDashboard") },
        ]}
      />,
    );
  }

  const tenantResult = await getSignInTenantContext(provider.provider);
  const requestedTenant = firstParam(params?.tenant);
  const codeChallenge = firstParam(params?.code_challenge) ?? "";
  const state = firstParam(params?.state) ?? "";
  const selectedTenant =
    tenantResult.tenants.find((tenant) => tenant.id === requestedTenant) ??
    (!requestedTenant && tenantResult.tenants.length === 1
      ? tenantResult.tenants[0]
      : null);
  const callbackParams = {
    redirect_url: redirectUrl,
    code_challenge: codeChallenge,
    state,
  };

  if (tenantResult.error) {
    return frame(
      <TicketSignInEmptyState
        title={t("tenantLookupTitle")}
        message={t("tenantLookupMessage")}
        statusLabel={t("actionRequired")}
        detail={tenantResult.error}
        detailLabel={t("developerDetails")}
        actions={[
          {
            href: `/cli-sign-in?${new URLSearchParams(callbackParams)}`,
            label: t("retry"),
          },
          { href: "/", label: t("returnDashboard") },
        ]}
      />,
    );
  }
  if (tenantResult.tenants.length === 0) {
    return frame(
      <TicketSignInEmptyState
        title={t("noTenantsTitle")}
        message={t("cliNoTenantsMessage")}
        statusLabel={t("actionRequired")}
        actions={[
          { href: "/#admin", label: t("openAdmin") },
          { href: "/", label: t("returnDashboard") },
        ]}
      />,
    );
  }
  if (!selectedTenant) {
    return frame(
      <TenantTicketSelection
        action="/cli-sign-in"
        continueLabel={t("continue")}
        hiddenFields={callbackParams}
        selectionTitle={t("selectTenant")}
        tenantLabel={t("tenant")}
        tenants={tenantResult.tenants}
      />,
    );
  }
  return frame(
    <CliTicketForm
      action={createCliTicket}
      autoSelectedTenant={!requestedTenant && tenantResult.tenants.length === 1}
      codeChallenge={codeChallenge}
      redirectUrl={redirectUrl}
      selectedTenant={selectedTenant}
      state={state}
    />,
  );
}

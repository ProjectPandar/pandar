import type { ReactNode } from "react";
import { getTranslations } from "next-intl/server";

import { createWebTicket } from "../actions";
import { authSource } from "../api-auth";
import { authProviderConfig } from "../auth-provider";
import { firstParam, getSignInTenantContext } from "../dashboard-data";
import {
  TenantTicketSelection,
  TicketSignInEmptyState,
  TicketSignInFrame,
} from "../ticket-sign-in-ui";
import { WebTicketForm } from "./web-ticket-form";

type PageProps = {
  searchParams?: Promise<{
    tenant?: string | string[];
    redirect_url?: string | string[];
    code_challenge?: string | string[];
    state?: string | string[];
  }>;
};

export default async function WebSignInPage({ searchParams }: PageProps) {
  const [t, params] = await Promise.all([
    getTranslations("signIn"),
    searchParams,
  ]);
  const redirectUrl = firstParam(params?.redirect_url);
  const frame = (content: ReactNode) => (
    <TicketSignInFrame
      title={t("webTitle")}
      subtitle={t("webSubtitle")}
      meta={t("webMeta")}
    >
      {content}
    </TicketSignInFrame>
  );
  if (!redirectUrl) {
    return frame(
      <TicketSignInEmptyState
        title={t("webMissingRedirectTitle")}
        message={t("webMissingRedirectMessage")}
        statusLabel={t("actionRequired")}
        actions={[{ href: "/", label: t("returnDashboard") }]}
      />,
    );
  }

  const auth = await authSource();
  const provider = authProviderConfig();
  const tenantResult = await getSignInTenantContext(provider.provider);
  const requestedTenant = firstParam(params?.tenant);
  const codeChallenge = firstParam(params?.code_challenge) ?? "";
  const state = firstParam(params?.state) ?? "";
  const selectedTenant =
    tenantResult.tenants.find((tenant) => tenant.id === requestedTenant) ??
    (!requestedTenant && tenantResult.tenants.length === 1
      ? tenantResult.tenants[0]
      : null);
  const localNoAuthMode =
    auth.source === "none" &&
    provider.provider === "none" &&
    !tenantResult.error;
  const callbackParams = {
    redirect_url: redirectUrl,
    code_challenge: codeChallenge,
    state,
  };

  if (auth.source === "none" && !localNoAuthMode) {
    return frame(
      <TicketSignInEmptyState
        title={t("authUnavailableTitle")}
        message={t("webAuthMessage")}
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
            href: `/web-sign-in?${new URLSearchParams(callbackParams)}`,
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
        message={t("webNoTenantsMessage")}
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
        action="/web-sign-in"
        continueLabel={t("continue")}
        hiddenFields={callbackParams}
        selectionTitle={t("selectTenant")}
        tenantLabel={t("tenant")}
        tenants={tenantResult.tenants}
      />,
    );
  }
  return frame(
    <WebTicketForm
      action={createWebTicket}
      autoSelectedTenant={!requestedTenant && tenantResult.tenants.length === 1}
      codeChallenge={codeChallenge}
      redirectUrl={redirectUrl}
      selectedTenant={selectedTenant}
      state={state}
    />,
  );
}

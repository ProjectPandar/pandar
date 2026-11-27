"use client";

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { apiIdSegment } from "./api-path";
import type { Job, Tenant } from "./dashboard-types";
import { invalidateTenantResources, mutationResources } from "./mutation-invalidation";
import { resourceDataKeys } from "./route-data";

export type JobDeleteState = {
  target: Job | null;
  deleting: boolean;
  error: boolean;
  request: (job: Job) => void;
  close: () => void;
  confirm: () => void;
};

/** Optimistic job deletion shared by the jobs page and per-printer queues. */
export function useJobDelete(
  selectedTenant: Tenant | null,
  onRedirect: (url: string) => void,
): JobDeleteState {
  const queryClient = useQueryClient();
  const [target, setTarget] = useState<Job | null>(null);
  const [error, setError] = useState(false);

  const deleteMutation = useMutation({
    mutationFn: async (jobId: string) => {
      const response = await fetch(
        `/api/tenants/${apiIdSegment(selectedTenant!.id, "tenant_id")}/jobs/${apiIdSegment(jobId, "job_id")}`,
        { method: "DELETE" },
      );
      if (!response.ok) {
        throw new Error("delete failed");
      }
      return response;
    },
    onMutate: async (jobId) => {
      const queryKey = resourceDataKeys.jobs(selectedTenant!.id);
      await queryClient.cancelQueries({ queryKey });
      const previousData = queryClient.getQueryData<Job[]>(queryKey);
      queryClient.setQueryData<Job[]>(queryKey, (old) =>
        old?.filter((job) => job.id !== jobId),
      );
      return { previousData };
    },
    onSuccess: async () => {
      setTarget(null);
      await invalidateTenantResources(
        queryClient,
        selectedTenant!.id,
        mutationResources.job,
      );
      onRedirect("/jobs?status=job_deleted");
    },
    onError: (_error, _jobId, context) => {
      if (context?.previousData) {
        queryClient.setQueryData(
          resourceDataKeys.jobs(selectedTenant!.id),
          context.previousData,
        );
      }
      setError(true);
    },
  });

  return {
    target,
    deleting: deleteMutation.isPending,
    error,
    request: (job) => {
      setError(false);
      setTarget(job);
    },
    close: () => {
      if (!deleteMutation.isPending) {
        setTarget(null);
        setError(false);
      }
    },
    confirm: () => {
      if (target) {
        deleteMutation.mutate(target.id);
      }
    },
  };
}

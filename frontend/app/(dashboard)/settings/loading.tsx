import { Skeleton } from "@/components/ui/skeleton";

export default function SettingsLoading() {
  return (
    <div className="mx-auto max-w-3xl pb-10">
      <div className="space-y-2 border-b border-border pb-6">
        <Skeleton className="h-8 w-36 rounded-lg" />
        <Skeleton className="h-4 w-72 rounded-lg" />
      </div>
      <div className="divide-y divide-border">
        <div className="py-5">
          <Skeleton className="h-14 rounded-lg" />
        </div>
        <div className="py-5">
          <Skeleton className="h-24 rounded-lg" />
        </div>
        <div className="py-5">
          <Skeleton className="h-16 rounded-lg" />
        </div>
      </div>
    </div>
  );
}

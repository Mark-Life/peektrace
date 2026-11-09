/** Render an Effect-Atom `Result` discriminated union.
 *
 * `Result` is the loading/success/failure union the RPC query atoms produce.
 * This collapses the three branches into a single declarative surface so screens
 * never juggle `isLoading` / `error` flag bags. `waiting` (a refetch over stale
 * data) renders the stale value untouched — freshness is signalled once, quietly,
 * by `WatchStatus` in the shell header, never by animating the content.
 */
import { Result } from "@effect-atom/atom-react";
import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@workspace/ui/components/alert";
import { Button } from "@workspace/ui/components/button";
import { Skeleton } from "@workspace/ui/components/skeleton";
import { Cause } from "effect";
import { RotateCwIcon } from "lucide-react";
import type { ReactNode } from "react";
import { wireErrorMessage, wireErrorOfCause } from "./wire-error";

/** Default skeleton shown while a query is in its initial (no-data) load. */
const DefaultPending = () => (
  <div className="flex flex-col gap-3" data-testid="result-pending">
    <Skeleton className="h-8 w-48" />
    <Skeleton className="h-32 w-full" />
    <Skeleton className="h-32 w-full" />
  </div>
);

/** Outline Retry button that spins and disables while the refetch runs. */
export const RetryButton = ({
  onRetry,
  retrying,
  testId,
  className,
}: {
  readonly onRetry: () => void;
  readonly retrying: boolean;
  readonly testId: string;
  readonly className?: string;
}) => (
  <Button
    className={className}
    data-testid={testId}
    disabled={retrying}
    onClick={onRetry}
    size="sm"
    variant="outline"
  >
    <RotateCwIcon
      aria-hidden
      className={retrying ? "motion-safe:animate-spin" : undefined}
    />
    {retrying ? "Retrying…" : "Retry"}
  </Button>
);

/**
 * Render a failed query. When the `Cause` carries a typed Peektrace wire error we
 * show its friendly message; otherwise (defects, transport faults) we fall back
 * to the pretty-printed cause so nothing is ever swallowed or blank-screened.
 */
const FailureView = ({
  cause,
  onRetry,
  retrying,
}: {
  readonly cause: Cause.Cause<unknown>;
  readonly onRetry?: () => void;
  readonly retrying: boolean;
}) => {
  const wire = wireErrorOfCause(cause);
  return (
    <Alert data-testid="result-error" variant="destructive">
      <AlertTitle>
        {wire ? "Something went wrong" : "Request failed"}
      </AlertTitle>
      <AlertDescription>
        {wire ? (
          <p className="text-sm">{wireErrorMessage(wire)}</p>
        ) : (
          <pre className="whitespace-pre-wrap break-words text-xs">
            {Cause.pretty(cause)}
          </pre>
        )}
        {onRetry ? (
          <RetryButton
            className="mt-2 text-foreground"
            onRetry={onRetry}
            retrying={retrying}
            testId="result-retry"
          />
        ) : null}
      </AlertDescription>
    </Alert>
  );
};

/**
 * Branch a `Result<A, E>` into pending / failure / success renderers.
 * `children` receives the success value; `pending` overrides the skeleton;
 * `onRetry` adds a Retry button to the failure view.
 */
export const ResultView = <A, E>({
  result,
  children,
  pending,
  onRetry,
}: {
  readonly result: Result.Result<A, E>;
  readonly children: (value: A) => ReactNode;
  readonly pending?: ReactNode;
  readonly onRetry?: () => void;
}) => {
  if (Result.isFailure(result)) {
    return (
      <FailureView
        cause={result.cause}
        onRetry={onRetry}
        retrying={result.waiting}
      />
    );
  }
  if (Result.isSuccess(result)) {
    // A refetch over data already on screen renders that data and nothing else:
    // no wrapper, no pulse, no remount. `result.waiting` is reported by the
    // `WatchStatus` dot in the shell header instead — dimming a live transcript
    // once per agent write is what made it unreadable.
    return <>{children(result.value)}</>;
  }
  return <>{pending ?? <DefaultPending />}</>;
};

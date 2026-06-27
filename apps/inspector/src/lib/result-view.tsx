/** Render an Effect-Atom `Result` discriminated union.
 *
 * `Result` is the loading/success/failure union the RPC query atoms produce.
 * This collapses the three branches into a single declarative surface so screens
 * never juggle `isLoading` / `error` flag bags. `waiting` (a refetch over stale
 * data) re-uses the success renderer with a subtle pulse rather than blanking.
 */
import { Result } from "@effect-atom/atom-react";
import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@workspace/ui/components/alert";
import { Skeleton } from "@workspace/ui/components/skeleton";
import { Cause } from "effect";
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

/**
 * Render a failed query. When the `Cause` carries a typed Peephole wire error we
 * show its friendly message; otherwise (defects, transport faults) we fall back
 * to the pretty-printed cause so nothing is ever swallowed or blank-screened.
 */
const FailureView = ({ cause }: { cause: Cause.Cause<unknown> }) => {
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
      </AlertDescription>
    </Alert>
  );
};

/**
 * Branch a `Result<A, E>` into pending / failure / success renderers.
 * `children` receives the success value; `pending` overrides the skeleton.
 */
export const ResultView = <A, E>({
  result,
  children,
  pending,
}: {
  readonly result: Result.Result<A, E>;
  readonly children: (value: A) => ReactNode;
  readonly pending?: ReactNode;
}) => {
  if (Result.isFailure(result)) {
    return <FailureView cause={result.cause} />;
  }
  if (Result.isSuccess(result)) {
    return (
      <div className={result.waiting ? "animate-pulse" : undefined}>
        {children(result.value)}
      </div>
    );
  }
  return <>{pending ?? <DefaultPending />}</>;
};

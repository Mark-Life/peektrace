/** Effect ↔ React bridge for the terminal UI.
 *
 * The TUI renders with `@opentui/react` (a plain React reconciler over the
 * terminal), but all data comes from the same in-process Effect-RPC client the
 * CLI uses — driven against the very handlers layer the sibling HTTP server
 * mounts, so the web app and the terminal read one coherent backend. This module
 * erases Effect at the React boundary: components get a promise-returning `run`,
 * a `useQuery` hook (loading/data/error, refetchable), and a single shared
 * `watch.poll` ticker for filesystem-driven live refresh.
 */
import type { makeInProcessClient, WatchVersions } from "@workspace/rpc";
import { type Effect, Runtime } from "effect";
import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";

/** The typed in-process client surface (same shape the CLI commands use). */
export type TuiClient = Effect.Effect.Success<
  ReturnType<typeof makeInProcessClient>
>;

/** The context the client's own effects require (the RPC/handlers services). */
export type TuiEnv =
  ReturnType<TuiClient["capabilities"]["list"]> extends Effect.Effect<
    unknown,
    unknown,
    infer R
  >
    ? R
    : never;

/** A query/mutation builder: given the client, produce the effect to run. */
export type Call<A, E> = (client: TuiClient) => Effect.Effect<A, E, TuiEnv>;

/** The runtime bridge handed to the React tree at the root. */
export interface TuiBridge {
  /** Run one client call to a promise (rejects with the typed failure). */
  readonly run: <A, E>(build: Call<A, E>) => Promise<A>;
  /** URL of the sibling web server, shown in the footer. */
  readonly serverUrl: string;
}

/**
 * Build the bridge from a live in-process client and the Effect runtime that
 * carries the provided handlers layer. `run` binds the client into a builder and
 * drives the resulting effect on that runtime.
 */
export const makeTuiBridge = (
  client: TuiClient,
  runtime: Runtime.Runtime<TuiEnv>,
  serverUrl: string
): TuiBridge => ({
  serverUrl,
  run: (build) => Runtime.runPromise(runtime)(build(client)),
});

const BridgeContext = createContext<TuiBridge | null>(null);

/** Provide the bridge to the whole tree. */
export const BridgeProvider = ({
  bridge,
  children,
}: {
  readonly bridge: TuiBridge;
  readonly children: ReactNode;
}) => <BridgeContext value={bridge}>{children}</BridgeContext>;

/** Access the bridge (throws if used outside the provider). */
export const useBridge = (): TuiBridge => {
  const bridge = useContext(BridgeContext);
  if (bridge === null) {
    throw new Error("useBridge must be used within a BridgeProvider");
  }
  return bridge;
};

/** Loading/data/error snapshot for a single query, plus a manual refetch. */
export interface QueryResult<A> {
  readonly data: A | undefined;
  readonly error: Error | undefined;
  readonly loading: boolean;
  readonly refetch: () => void;
}

/**
 * Run a client query and track its result reactively. `deps` control refetching
 * exactly like `useEffect` deps — pass every value the `build` closure reads
 * (ids, filters, watch ticks). The `build` closure itself is intentionally not a
 * dependency, so it may be an inline arrow.
 */
export const useQuery = <A, E>(
  build: Call<A, E>,
  deps: readonly unknown[]
): QueryResult<A> => {
  const bridge = useBridge();
  const [nonce, setNonce] = useState(0);
  const [state, setState] = useState<Omit<QueryResult<A>, "refetch">>({
    loading: true,
    data: undefined,
    error: undefined,
  });
  const buildRef = useRef(build);
  buildRef.current = build;

  // `nonce` is the refetch trigger and `deps` the caller-declared inputs;
  // `build` is read through a ref so an inline closure never re-runs the query.
  // biome-ignore lint/correctness/useExhaustiveDependencies: refetch trigger + caller deps
  useEffect(() => {
    let cancelled = false;
    setState((prev) => ({ ...prev, loading: true, error: undefined }));
    bridge
      .run(buildRef.current)
      .then((data) => {
        if (!cancelled) {
          setState({ loading: false, data, error: undefined });
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setState({
            loading: false,
            data: undefined,
            error: err instanceof Error ? err : new Error(String(err)),
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [bridge, nonce, ...deps]);

  return { ...state, refetch: () => setNonce((n) => n + 1) };
};

const ZERO_VERSIONS: WatchVersions = { memory: 0, sessions: 0 };
const WATCH_INTERVAL_MS = 1000;

const WatchContext = createContext<WatchVersions>(ZERO_VERSIONS);

/**
 * Poll `watch.poll` once per second and publish the monotonic per-scope
 * versions. Screens read the relevant counter and thread it into their query
 * deps, so an on-disk change re-validates just that scope — mirroring the web
 * inspector's watch-driven refresh over the same NDJSON transport.
 */
export const WatchProvider = ({
  children,
}: {
  readonly children: ReactNode;
}) => {
  const bridge = useBridge();
  const [versions, setVersions] = useState<WatchVersions>(ZERO_VERSIONS);

  useEffect(() => {
    let alive = true;
    const tick = () => {
      bridge
        .run((c) => c.watch.poll())
        .then((next) => {
          if (alive) {
            setVersions((prev) =>
              prev.memory === next.memory && prev.sessions === next.sessions
                ? prev
                : next
            );
          }
        })
        .catch(() => {
          /* transient; next tick retries */
        });
    };
    const id = setInterval(tick, WATCH_INTERVAL_MS);
    tick();
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [bridge]);

  return <WatchContext value={versions}>{children}</WatchContext>;
};

/** Current filesystem watch versions (thread into query deps to auto-refresh). */
export const useWatch = (): WatchVersions => useContext(WatchContext);

/** Empty session list: which folders were scanned, and where to add one. */
import { Result } from "@effect-atom/atom-react";
import type { ServerInfo } from "@workspace/rpc/contract";
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@workspace/ui/components/empty";
import { Skeleton } from "@workspace/ui/components/skeleton";
import { MessagesSquareIcon } from "lucide-react";
import { RootRow } from "../../components/root-row";
import { AGENT_LABEL } from "../../lib/agent-label";
import { RetryButton } from "../../lib/result-view";
import { useLiveServerInfo, withLastSuccess } from "../../lib/server-info";

const ScannedFolders = ({ info }: { readonly info: ServerInfo }) => (
  <ul className="flex w-full flex-col gap-1.5 text-left" data-testid="scanned">
    {info.agents.flatMap(({ agent, roots }) =>
      roots.map((root) => (
        <li key={`${agent}:${root.path}`}>
          <RootRow
            exists={root.exists}
            home={info.userHome}
            label={AGENT_LABEL[agent]}
            path={root.path}
          />
        </li>
      ))
    )}
  </ul>
);

const FoldersPending = () => (
  <div className="flex w-full flex-col gap-1.5" data-testid="scanned-pending">
    <Skeleton className="h-4 w-3/4" />
    <Skeleton className="h-4 w-2/3" />
  </div>
);

/** Pure view over the `server.info` result. */
export const NoSessions = ({
  info,
  onRetry,
}: {
  readonly info: Result.Result<ServerInfo, unknown>;
  readonly onRetry: () => void;
}) => (
  <Empty className="border" data-testid="sessions-placeholder">
    <EmptyHeader>
      <EmptyMedia variant="icon">
        <MessagesSquareIcon />
      </EmptyMedia>
      <EmptyTitle>No sessions found</EmptyTitle>
      <EmptyDescription>
        {Result.isFailure(info)
          ? "Peektrace found no transcripts and could not list the folders it scans."
          : "Peektrace looked for transcripts in these folders:"}
      </EmptyDescription>
    </EmptyHeader>
    <EmptyContent className="max-w-lg">
      {Result.isSuccess(info) ? <ScannedFolders info={info.value} /> : null}
      {Result.isInitial(info) ? <FoldersPending /> : null}
      {Result.isFailure(info) ? (
        <RetryButton
          onRetry={onRetry}
          retrying={info.waiting}
          testId="scanned-retry"
        />
      ) : null}
      <a
        className="text-sm underline underline-offset-3 hover:text-foreground"
        data-testid="sessions-settings-link"
        href="#/settings"
      >
        Add a folder in Settings
      </a>
    </EmptyContent>
  </Empty>
);

export const NoSessionsFromServer = () => {
  const { info, refresh } = useLiveServerInfo();
  return <NoSessions info={withLastSuccess(info)} onRetry={refresh} />;
};

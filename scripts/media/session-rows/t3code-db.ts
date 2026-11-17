// A t3code state DB for make-home.ts: the table schemas t3code uses, with
// fake rows only.
import { Database } from "bun:sqlite";

const SCHEMA = `CREATE TABLE projection_projects (
      project_id TEXT PRIMARY KEY,
      title TEXT NOT NULL,
      workspace_root TEXT NOT NULL,
      scripts_json TEXT NOT NULL,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      deleted_at TEXT
    , default_model_selection_json TEXT, default_thread_env_mode TEXT, favicon_path TEXT, auto_pull INTEGER NOT NULL DEFAULT 0, project_icon_json TEXT, pinned_at TEXT, archived_at TEXT, has_custom_title INTEGER NOT NULL DEFAULT 0);
CREATE TABLE orchestration_v2_projection_threads (
      thread_id TEXT PRIMARY KEY,
      project_id TEXT NOT NULL,
      title TEXT NOT NULL,
      default_provider TEXT NOT NULL,
      runtime_mode TEXT NOT NULL,
      interaction_mode TEXT NOT NULL,
      active_provider_thread_id TEXT,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      archived_at TEXT,
      deleted_at TEXT,
      payload_json TEXT NOT NULL
    , provider_instance_id TEXT);
CREATE TABLE orchestration_v2_projection_provider_threads (
      provider_thread_id TEXT PRIMARY KEY,
      thread_id TEXT,
      owner_node_id TEXT,
      provider TEXT NOT NULL,
      provider_session_id TEXT,
      status TEXT NOT NULL,
      first_run_ordinal INTEGER,
      last_run_ordinal INTEGER,
      updated_at TEXT NOT NULL,
      payload_json TEXT NOT NULL
    , driver TEXT, provider_instance_id TEXT);`;

export interface T3Project {
  readonly id: string;
  readonly title: string;
  readonly root: string;
}

export interface T3Thread {
  readonly id: string;
  readonly title: string;
  readonly branch: string;
  readonly worktreePath: string | null;
  readonly parentId?: string;
  /** The Claude session id the thread ran. */
  readonly nativeId: string;
}

const CREATED = "2026-11-16T08:00:00.000Z";
const DRIVER = "claudeAgent";

export const writeT3Db = (
  file: string,
  project: T3Project,
  threads: readonly T3Thread[]
) => {
  const db = new Database(file, { create: true });

  db.run(SCHEMA);
  db.query(
    "INSERT INTO projection_projects (project_id, title, workspace_root, scripts_json, created_at, updated_at) VALUES (?, ?, ?, '[]', ?, ?)"
  ).run(project.id, project.title, project.root, CREATED, CREATED);

  for (const th of threads) {
    const lineage =
      th.parentId === undefined
        ? null
        : {
            parentThreadId: th.parentId,
            relationshipToParent: "subagent",
            rootThreadId: th.parentId,
          };

    const payload = {
      branch: th.branch,
      createdAt: CREATED,
      id: th.id,
      lineage,
      modelSelection: {
        instanceId: DRIVER,
        model: "claude-opus-4-8",
        options: [],
      },
      projectId: project.id,
      providerInstanceId: DRIVER,
      title: th.title,
      updatedAt: CREATED,
      worktreePath: th.worktreePath,
    };

    const provider = {
      appThreadId: th.id,
      driver: DRIVER,
      id: `pt-${th.id}`,
      nativeThreadRef: {
        driver: DRIVER,
        nativeId: th.nativeId,
        strength: "strong",
      },
      providerInstanceId: DRIVER,
      status: "idle",
    };

    db.query(
      "INSERT INTO orchestration_v2_projection_threads (thread_id, project_id, title, default_provider, runtime_mode, interaction_mode, created_at, updated_at, payload_json, provider_instance_id) VALUES (?, ?, ?, ?, 'full-access', 'default', ?, ?, ?, ?)"
    ).run(
      th.id,
      project.id,
      th.title,
      DRIVER,
      CREATED,
      CREATED,
      JSON.stringify(payload),
      DRIVER
    );
    db.query(
      "INSERT INTO orchestration_v2_projection_provider_threads (provider_thread_id, thread_id, provider, status, updated_at, payload_json, driver, provider_instance_id) VALUES (?, ?, ?, 'idle', ?, ?, ?, ?)"
    ).run(
      `pt-${th.id}`,
      th.id,
      DRIVER,
      CREATED,
      JSON.stringify(provider),
      DRIVER,
      DRIVER
    );
  }

  db.close();
};

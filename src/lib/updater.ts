import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { ipc } from "./ipc";

export type CheckResult =
  | { kind: "up-to-date" }
  | { kind: "available"; update: Update }
  /** Almost always offline, or the endpoint isn't published yet. */
  | { kind: "failed"; error: string };

export const currentVersion = getVersion;

export async function checkForUpdate(): Promise<CheckResult> {
  try {
    const update = await check();
    return update ? { kind: "available", update } : { kind: "up-to-date" };
  } catch (e) {
    return { kind: "failed", error: String(e) };
  }
}

export interface InstallProgress {
  /** 0–1, or null while the server hasn't given a content length. */
  fraction: number | null;
  downloaded: number;
  total: number | null;
}

/**
 * Back up, then download and install. The backup is deliberately not
 * best-effort: if we can't snapshot the database we don't get to touch the
 * app that owns it, so the failure propagates and nothing is installed.
 */
export async function backupAndInstall(
  update: Update,
  onProgress: (p: InstallProgress) => void
): Promise<void> {
  await ipc.backupCreate(`before-${update.version}`);

  let downloaded = 0;
  let total: number | null = null;

  await update.downloadAndInstall((event) => {
    if (event.event === "Started") {
      total = event.data.contentLength ?? null;
      downloaded = 0;
    } else if (event.event === "Progress") {
      downloaded += event.data.chunkLength;
    } else if (event.event === "Finished") {
      downloaded = total ?? downloaded;
    }
    onProgress({
      fraction: total ? Math.min(downloaded / total, 1) : null,
      downloaded,
      total,
    });
  });
}

export async function restartToInstall(): Promise<void> {
  await relaunch();
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

import { CapsuleStatus } from "./manifest";

export interface StatusBadge {
	/** Short human-readable label, e.g. "active". */
	label: string;
	/** CSS class applied to the badge element for color theming. */
	cssClass: string;
	/** Small leading glyph shown before the label. */
	glyph: string;
}

const BADGE_BY_STATUS: Record<CapsuleStatus, StatusBadge> = {
	active: { label: "active", cssClass: "capsule-status-active", glyph: "●" },
	paused: { label: "paused", cssClass: "capsule-status-paused", glyph: "◌" },
	draft: { label: "draft", cssClass: "capsule-status-draft", glyph: "○" },
	archived: {
		label: "archived",
		cssClass: "capsule-status-archived",
		glyph: "✕",
	},
};

export function statusBadge(status: CapsuleStatus): StatusBadge {
	return BADGE_BY_STATUS[status];
}

/**
 * True iff `path` points to a note that lives under the configured capsule
 * folder. Path semantics match the vault adapter's normalized form —
 * forward slashes, no leading slash.
 */
export function isCapsuleNotePath(path: string, capsuleFolder: string): boolean {
	if (!path.endsWith(".md")) return false;
	const folder = capsuleFolder.endsWith("/") ? capsuleFolder : `${capsuleFolder}/`;
	return path.startsWith(folder);
}

/**
 * Extract a capsule status from a frontmatter object (as Obsidian's
 * metadataCache exposes it). Returns null when the frontmatter is missing
 * or the status field is absent or invalid — callers treat null as "not a
 * rendered capsule" and skip the decoration.
 */
export function statusFromFrontmatter(
	frontmatter: Record<string, unknown> | null | undefined
): CapsuleStatus | null {
	if (!frontmatter) return null;
	const raw = frontmatter.status;
	if (raw === "active" || raw === "paused" || raw === "draft" || raw === "archived") {
		return raw;
	}
	return null;
}

import { parse as parseYaml, stringify as stringifyYaml } from "yaml";
import {
	CapsuleStatus,
	ComputationClass,
	ExtractionMode,
	isValidCapsuleId,
	Manifest,
} from "./manifest";

// Note: Obsidian also exports parseYaml/stringifyYaml, which are thin
// wrappers over the same `yaml` package. Importing the library directly
// lets this module run under `node --test` without an Obsidian runtime.

export const ZONE_MARKER =
	"# ═══ Computed Fields (daemon-managed, read-only) ═══";

const FRONTMATTER_FENCE = "---";

export interface DaemonManagedFields {
	payload_cid?: string | null;
	earnings_total?: string | null;
	queries_served?: number | null;
	last_accessed?: string | null;
}

export interface ParsedNote {
	manifest: Manifest;
	daemonFields: DaemonManagedFields;
	body: string;
}

export class FrontmatterError extends Error {
	constructor(message: string) {
		super(message);
		this.name = "FrontmatterError";
	}
}

/**
 * Split a full note's content into the raw frontmatter body (between `---`
 * fences) and the remaining body. Returns null if the file has no frontmatter
 * — capsule notes must have one, so callers treat null as an error.
 */
function splitFrontmatter(
	content: string
): { frontmatter: string; body: string } | null {
	if (!content.startsWith(FRONTMATTER_FENCE)) return null;
	const afterOpen = content.indexOf("\n", FRONTMATTER_FENCE.length);
	if (afterOpen === -1) return null;
	const closeIdx = content.indexOf(
		`\n${FRONTMATTER_FENCE}`,
		afterOpen + 1
	);
	if (closeIdx === -1) return null;
	const frontmatter = content.slice(afterOpen + 1, closeIdx);
	const afterClose = content.indexOf("\n", closeIdx + 1);
	const body = afterClose === -1 ? "" : content.slice(afterClose + 1);
	return { frontmatter, body };
}

/**
 * Split frontmatter text into the user-editable zone (above the marker) and
 * the daemon-managed zone (below). Either zone may be empty. The marker line
 * itself is discarded — callers re-emit it when writing back.
 */
export function splitZones(frontmatter: string): {
	user: string;
	daemon: string;
} {
	const lines = frontmatter.split("\n");
	const markerIdx = lines.findIndex((line) => line.trim() === ZONE_MARKER);
	if (markerIdx === -1) {
		return { user: frontmatter, daemon: "" };
	}
	return {
		user: lines.slice(0, markerIdx).join("\n"),
		daemon: lines.slice(markerIdx + 1).join("\n"),
	};
}

function toStringArray(raw: unknown, field: string): string[] {
	if (raw == null) return [];
	if (!Array.isArray(raw)) {
		throw new FrontmatterError(`${field} must be an array`);
	}
	return raw.map((item, i) => {
		if (typeof item !== "string") {
			throw new FrontmatterError(`${field}[${i}] must be a string`);
		}
		return item;
	});
}

function toStatus(raw: unknown): CapsuleStatus {
	if (raw === "active" || raw === "paused" || raw === "draft" || raw === "archived") {
		return raw;
	}
	throw new FrontmatterError(
		`status must be one of active|paused|draft|archived`
	);
}

function toComputationClasses(raw: unknown): ComputationClass[] {
	const arr = toStringArray(raw, "computation_classes");
	return arr.map((item, i) => {
		if (item === "A" || item === "B" || item === "C") return item;
		throw new FrontmatterError(
			`computation_classes[${i}] must be A, B, or C`
		);
	});
}

function toExtractionMode(raw: unknown): ExtractionMode | undefined {
	if (raw == null) return undefined;
	if (raw === "none" || raw === "frontmatter-list") return raw;
	throw new FrontmatterError(
		`extraction must be one of: none, frontmatter-list`
	);
}

/**
 * Parse a capsule note's full content into a Manifest plus any
 * daemon-managed fields present. Throws FrontmatterError on malformed input.
 */
export function parseCapsuleNote(content: string): ParsedNote {
	const split = splitFrontmatter(content);
	if (!split) {
		throw new FrontmatterError("note has no `---` frontmatter block");
	}
	const { user, daemon } = splitZones(split.frontmatter);

	const userYaml = user.trim() ? (parseYaml(user) as Record<string, unknown>) : {};
	const daemonYaml = daemon.trim()
		? (parseYaml(daemon) as Record<string, unknown>)
		: {};

	if (typeof userYaml.capsule_id !== "string") {
		throw new FrontmatterError("capsule_id missing or not a string");
	}
	if (!isValidCapsuleId(userYaml.capsule_id)) {
		throw new FrontmatterError(
			`capsule_id ${userYaml.capsule_id} is not valid`
		);
	}
	if (typeof userYaml.schema !== "string") {
		throw new FrontmatterError("schema missing or not a string");
	}
	if (typeof userYaml.floor_price !== "string") {
		throw new FrontmatterError("floor_price missing or not a string");
	}

	const sources =
		userYaml.sources == null ? undefined : toStringArray(userYaml.sources, "sources");
	const extraction = toExtractionMode(userYaml.extraction);

	const manifest: Manifest = {
		capsule_id: userYaml.capsule_id,
		schema: userYaml.schema,
		status: toStatus(userYaml.status),
		floor_price: userYaml.floor_price,
		computation_classes: toComputationClasses(userYaml.computation_classes),
		tags: toStringArray(userYaml.tags ?? [], "tags"),
		...(sources !== undefined ? { sources } : {}),
		...(extraction !== undefined ? { extraction } : {}),
	};

	const daemonFields: DaemonManagedFields = {
		payload_cid: normalizeNullableString(daemonYaml.payload_cid, "payload_cid"),
		earnings_total: normalizeNullableString(
			daemonYaml.earnings_total,
			"earnings_total"
		),
		queries_served: normalizeNullableNumber(
			daemonYaml.queries_served,
			"queries_served"
		),
		last_accessed: normalizeNullableString(
			daemonYaml.last_accessed,
			"last_accessed"
		),
	};

	return { manifest, daemonFields, body: split.body };
}

function normalizeNullableString(raw: unknown, field: string): string | null {
	if (raw == null) return null;
	if (typeof raw !== "string") {
		throw new FrontmatterError(`${field} must be a string or null`);
	}
	return raw;
}

function normalizeNullableNumber(raw: unknown, field: string): number | null {
	if (raw == null) return null;
	if (typeof raw !== "number" || !Number.isFinite(raw)) {
		throw new FrontmatterError(`${field} must be a number or null`);
	}
	return raw;
}

/**
 * Build a full capsule-note string from a manifest and (optional) daemon
 * fields + body. Emits both zones with the marker between them. Used when
 * creating a new note; later slices will call this to rewrite the daemon
 * zone while preserving the user zone unchanged (via replaceDaemonZone).
 */
export function buildCapsuleNote(params: {
	manifest: Manifest;
	daemonFields?: DaemonManagedFields;
	body?: string;
}): string {
	const userYaml = stringifyYaml({
		capsule_id: params.manifest.capsule_id,
		schema: params.manifest.schema,
		status: params.manifest.status,
		floor_price: params.manifest.floor_price,
		computation_classes: params.manifest.computation_classes,
		tags: params.manifest.tags,
		sources: params.manifest.sources ?? [],
		extraction: params.manifest.extraction ?? "none",
	}).trimEnd();

	const daemon = params.daemonFields ?? {
		payload_cid: null,
		earnings_total: null,
		queries_served: null,
		last_accessed: null,
	};
	const daemonYaml = stringifyYaml(daemon).trimEnd();

	const body = params.body ?? defaultBody(params.manifest);

	return [
		FRONTMATTER_FENCE,
		userYaml,
		ZONE_MARKER,
		daemonYaml,
		FRONTMATTER_FENCE,
		"",
		body,
	].join("\n");
}

function defaultBody(manifest: Manifest): string {
	return [
		`# ${manifest.capsule_id}`,
		"",
		"## Description",
		"",
		"_Describe what this capsule exposes and under what terms._",
		"",
		"## Data sources",
		"",
		"- _Add the sources this capsule draws from._",
		"",
		"## Policy",
		"",
		"- _Link the policy note that governs access._",
		"",
	].join("\n");
}

/**
 * Rewrite just the daemon-managed zone of an existing note, leaving the
 * user-editable zone and body untouched. If the note has no marker yet, the
 * marker + daemon zone are inserted at the end of the frontmatter.
 *
 * Not called yet in this slice — the daemon doesn't produce computed fields.
 * Defined here so the boundary is enforced on day one: any future write path
 * that updates daemon-owned fields goes through this function.
 */
export function replaceDaemonZone(
	content: string,
	daemonFields: DaemonManagedFields
): string {
	const split = splitFrontmatter(content);
	if (!split) {
		throw new FrontmatterError("note has no `---` frontmatter block");
	}
	const { user } = splitZones(split.frontmatter);
	const daemonYaml = stringifyYaml(daemonFields).trimEnd();
	const newFrontmatter = [
		user.trimEnd(),
		ZONE_MARKER,
		daemonYaml,
	].join("\n");
	return [
		FRONTMATTER_FENCE,
		newFrontmatter,
		FRONTMATTER_FENCE,
		"",
		split.body,
	].join("\n");
}

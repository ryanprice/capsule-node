export type CapsuleStatus = "active" | "paused" | "draft" | "archived";
export type ComputationClass = "A" | "B" | "C";

/**
 * How the plugin extracts data from the capsule's source notes into
 * schema-typed records. See capsule-node/design/001-data-sharing.md in
 * the vault for the full rationale.
 *
 * Modes:
 *  * `none`           — no pipeline (default for paused/draft capsules)
 *  * `frontmatter-list` — each source note's YAML frontmatter is one record
 *  * `table`          — the first markdown table in each source note is a
 *                       batch of records (headers → column names)
 *  * `code-fence`     — every ```capsule-data[:lang]``` block in a source
 *                       is parsed (lang = json | csv; default json) and
 *                       its entries concatenated into the record stream
 */
export type ExtractionMode =
	| "none"
	| "frontmatter-list"
	| "table"
	| "code-fence";

export interface Manifest {
	capsule_id: string;
	schema: string;
	status: CapsuleStatus;
	floor_price: string;
	computation_classes: ComputationClass[];
	tags: string[];
	/** Source notes whose content this capsule exposes. Each entry is
	 * either a wikilink (`"[[path]]"` or `"[[path|alias]]"`) or a plain
	 * vault-relative path. Absent / empty means "no data yet — drafts". */
	sources?: string[];
	/** How to turn the source notes into records. Defaults to `none`
	 * (capsule has no data pipeline yet). */
	extraction?: ExtractionMode;
}

const CAP_ID_SUFFIX_ALPHABET = "abcdefghijklmnopqrstuvwxyz0123456789";

/**
 * Generate a random `cap_xxxxxx` identifier.
 *
 * Uses Web Crypto (`crypto.getRandomValues`) — available in Obsidian's
 * Electron renderer. The suffix is a 6-character base-36-ish string,
 * giving ~2^31 distinct ids, ample collision resistance at this scale.
 */
export function generateCapsuleId(): string {
	const buf = new Uint8Array(6);
	crypto.getRandomValues(buf);
	let suffix = "";
	for (const byte of buf) {
		suffix += CAP_ID_SUFFIX_ALPHABET[byte % CAP_ID_SUFFIX_ALPHABET.length];
	}
	return `cap_${suffix}`;
}

/**
 * Validation mirror of the daemon's CapsuleId rules — lowercase alnum,
 * 1–32 chars after the `cap_` prefix. The daemon re-validates on read;
 * this is for friendly client-side feedback.
 */
export function isValidCapsuleId(id: string): boolean {
	if (!id.startsWith("cap_")) return false;
	const suffix = id.slice(4);
	if (suffix.length === 0 || suffix.length > 32) return false;
	return /^[a-z0-9]+$/.test(suffix);
}
